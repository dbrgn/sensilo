#![no_main]
#![cfg_attr(not(test), no_std)]

// Panic handler
#[cfg(not(test))]
use panic_rtt_target as _;

use core::cmp::max;

use nrf52832_hal::{self as hal, pac, prelude::*};
use rtic::app;
use rtt_target::{rprintln, rtt_init_print};
use rubble::{
    beacon::Beacon,
    link::{ad_structure::AdStructure, DeviceAddress, MIN_PDU_BUF},
};
use rubble_nrf5x::{
    radio::{BleRadio, PacketBuffer},
    utils::get_device_address,
};
use shared_bus_rtic::SharedBus;
use shtcx::{shtc3, ShtC3};
use veml6030::Veml6030;

mod monotonic_nrf52;

use monotonic_nrf52::{Instant, U32Ext};

// Measure at a specific interval
const MEASURE_INTERVAL_MS: u32 = 3000;

// Send 3 beacons, spaced 20 ms apart
const BEACON_BURST_COUNT: u8 = 3;
const BEACON_BURST_INTERVAL_MS: u32 = 20;

// Sensor types
const SENSOR_TEMP: u8 = 0x01;
const SENSOR_HUMI: u8 = 0x02;
const SENSOR_LUX: u8 = 0x04;

// BLE Beacon
const AD_STRUCTURE_MANUFACTURER_DATA: u8 = 0xff;

// VEML sensor integration time
const VEML_INTEGRATION_TIME: veml6030::IntegrationTime = veml6030::IntegrationTime::Ms25;

pub struct SharedBusResources<T: 'static> {
    sht: ShtC3<SharedBus<T>>,
    veml: Veml6030<SharedBus<T>>,
}

type SharedBusType = hal::twim::Twim<pac::TWIM0>;

#[app(device = crate::pac, peripherals = true, monotonic = crate::monotonic_nrf52::Tim1)]
const APP: () = {
    struct Resources {
        // LED
        led: hal::gpio::p0::P0_07<hal::gpio::Output<hal::gpio::PushPull>>,

        // BLE
        #[init([0; MIN_PDU_BUF])]
        ble_tx_buf: PacketBuffer,
        #[init([0; MIN_PDU_BUF])]
        ble_rx_buf: PacketBuffer,
        radio: BleRadio,
        device_address: DeviceAddress,

        // I²C devices
        i2c: SharedBusResources<SharedBusType>,

        // Measurements
        #[init(None)]
        measurement_start: Option<Instant>,

        // Beacon
        #[init(None)]
        beacon: Option<Beacon>,
    }

    #[init(resources = [ble_tx_buf, ble_rx_buf], spawn = [start_measurement])]
    fn init(ctx: init::Context) -> init::LateResources {
        // Init RTT
        rtt_init_print!();
        rprintln!("Initializing…");

        // Destructure device peripherals
        let pac::Peripherals {
            CLOCK,
            FICR,
            P0,
            RADIO,
            TIMER1,
            TWIM0,
            ..
        } = ctx.device;

        // Set up clocks. On reset, the high frequency clock is already used,
        // but we also need to switch to the external HF oscillator. This is
        // needed for Bluetooth to work.
        let _clocks = hal::clocks::Clocks::new(CLOCK).enable_ext_hfosc();

        // Set up GPIO peripheral
        let gpio = hal::gpio::p0::Parts::new(P0);

        // Initialize monotonic timer on TIMER1 (for RTIC)
        monotonic_nrf52::Tim1::initialize(TIMER1);

        // Initialize LED pin
        // TODO: LED wrapper that knows whether low power mode is enabled
        let led = gpio.p0_07.into_push_pull_output(hal::gpio::Level::High);

        // Initialize TWIM (I²C) peripheral
        let sda = gpio.p0_26.into_floating_input().degrade();
        let scl = gpio.p0_25.into_floating_input().degrade();
        let twim = hal::twim::Twim::new(
            TWIM0,
            hal::twim::Pins { sda, scl },
            hal::twim::Frequency::K250,
        );

        // Create shared bus
        let bus_manager = shared_bus_rtic::new!(twim, SharedBusType);

        // Initialize SHT sensor
        let mut sht = shtc3(bus_manager.acquire());
        rprintln!(
            "SHTC3: Device identifier is {}",
            sht.device_identifier().unwrap()
        );

        // Initialize VEML7700 lux sensor
        let mut veml = Veml6030::new(bus_manager.acquire(), veml6030::SlaveAddr::default());
        if let Err(e) = veml.set_gain(veml6030::Gain::One) {
            rprintln!("VEML7700: Could not set gain: {:?}", e);
        }
        if let Err(e) = veml.set_integration_time(VEML_INTEGRATION_TIME) {
            rprintln!("VEML7700: Could not set gain: {:?}", e);
        }

        // Get bluetooth device address
        let device_address = get_device_address();
        rprintln!("Bluetooth device address: {:?}", device_address);

        // Initialize radio
        let radio = BleRadio::new(
            RADIO,
            &FICR,
            ctx.resources.ble_tx_buf,
            ctx.resources.ble_rx_buf,
        );

        // Schedule measurement immediately
        ctx.spawn.start_measurement().unwrap();

        rprintln!("Init done");
        init::LateResources {
            radio,
            device_address,
            i2c: SharedBusResources { sht, veml },
            led,
        }
    }

    /// Start a measurement
    #[task(resources = [i2c, measurement_start], schedule = [collect_measurement])]
    fn start_measurement(ctx: start_measurement::Context) {
        let i2c = ctx.resources.i2c;
        let power_mode = shtcx::PowerMode::NormalMode;

        // Store the instant when this task was scheduled.
        // This ensures that there is no jitter in scheduling.
        *ctx.resources.measurement_start = Some(ctx.scheduled);

        // Trigger SHTC3 measurement
        i2c.sht.start_measurement(power_mode).unwrap();
        let sht_delta_us: u32 = shtcx::max_measurement_duration(&i2c.sht, power_mode) as u32;

        // Turn on VEML7700
        //
        // Note: After enabling the sensor, a startup time of 4 ms plus the integration time must
        // be awaited.
        if let Err(e) = i2c.veml.enable() {
            rprintln!("VEML7700: Could not enable sensor: {:?}", e);
        }
        let veml_delta_us: u32 = VEML_INTEGRATION_TIME.as_us() + 4_000;

        // Calculate timedelta until collection
        let timedelta = max(sht_delta_us, veml_delta_us).micros();

        // Schedule measurement collection
        ctx.schedule
            .collect_measurement(Instant::now() + timedelta)
            .unwrap();
    }

    /// Collect a measurement. Then send the data using non-connectable BLE
    /// advertisement frames (beacons).
    #[task(
        resources = [i2c, measurement_start, device_address, beacon],
        schedule = [start_measurement],
        spawn = [broadcast_beacon],
    )]
    fn collect_measurement(ctx: collect_measurement::Context) {
        static mut COUNTER: u16 = 0;

        let i2c = ctx.resources.i2c;

        // Take measurement start time
        let measurement_start = ctx
            .resources
            .measurement_start
            .take()
            .expect("Cannot collect measurement without starting a measurement first");

        // Collect SHTC3 measurement result
        let sht_measurement = i2c.sht.get_measurement_result().unwrap();
        rprintln!(
            "SHTC3 measurement: {}°C / {} %RH",
            sht_measurement.temperature.as_degrees_celsius(),
            sht_measurement.humidity.as_percent()
        );

        // Collect VEML7700 measurement result
        let veml_measurement = match i2c.veml.read_lux() {
            Ok(lux) => {
                rprintln!("VEML7700 measurement: {:.1} lx", lux);
                Some(lux)
            }
            Err(e) => {
                rprintln!("VEML7700: Could not measure lux: {:?}", e);
                None
            }
        };

        // Prepare beacon payload
        let temp = sht_measurement
            .temperature
            .as_millidegrees_celsius()
            .to_le_bytes();
        let humi = sht_measurement.humidity.as_millipercent().to_le_bytes();
        let lux = veml_measurement
            .expect("TODO: Allow VEML measurement errors")
            .to_le_bytes();
        let counter_bytes = COUNTER.to_le_bytes();
        #[rustfmt::skip]
        let payload = [
            0xff, 0xff,
            counter_bytes[0], counter_bytes[1],
            SENSOR_TEMP, temp[0], temp[1], temp[2], temp[3], // i32 LE
            SENSOR_HUMI, humi[0], humi[1], humi[2], humi[3], // i32 LE
            SENSOR_LUX, lux[0], lux[1], lux[2], lux[3], // f32 LE
        ];

        // Create beacon
        let advertisement_data = [
            AdStructure::CompleteLocalName("Sensilo"),
            AdStructure::Unknown {
                ty: AD_STRUCTURE_MANUFACTURER_DATA,
                data: &payload,
            },
        ];
        let beacon = Beacon::new(*ctx.resources.device_address, &advertisement_data)
            .expect("Could not create beacon");
        *ctx.resources.beacon = Some(beacon);
        rprintln!("Created beacon with counter {}", COUNTER);

        // Broadcast beacon
        if ctx.spawn.broadcast_beacon(0).is_err() {
            rprintln!("Error: Could not spawn broadcast_beacon");
        }

        // Increment counter (allow wrap-around)
        *COUNTER = COUNTER.wrapping_add(1);

        // Schedule a new measurement
        ctx.schedule
            .start_measurement(measurement_start + MEASURE_INTERVAL_MS.millis())
            .unwrap();
    }

    /// Broadcast the beacon until the BEACON_BURST_COUNT has been reached.
    #[task(resources = [radio, beacon, led], schedule = [broadcast_beacon])]
    fn broadcast_beacon(ctx: broadcast_beacon::Context, i: u8) {
        if i == 0 {
            ctx.resources.led.set_low().ok();
        } else if i >= BEACON_BURST_COUNT {
            ctx.resources.led.set_high().ok();
            return;
        }

        if let Some(beacon) = ctx.resources.beacon {
            beacon.broadcast(ctx.resources.radio);
            rprintln!("Sent beacon");

            if ctx
                .schedule
                .broadcast_beacon(ctx.scheduled + BEACON_BURST_INTERVAL_MS.millis(), i + 1)
                .is_err()
            {
                rprintln!("Error: Could not re-schedule broadcast_beacon");
            }
        } else {
            rprintln!("Error: No beacon that can be broadcasted");
        }
    }

    // Provide unused interrupts to RTIC for its scheduling
    extern "C" {
        fn SWI0_EGU0();
        fn SWI1_EGU1();
        fn SWI2_EGU2();
        fn SWI3_EGU3();
        fn SWI4_EGU4();
        fn SWI5_EGU5();
    }
};
