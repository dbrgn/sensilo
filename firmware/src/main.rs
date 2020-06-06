#![no_main]
#![cfg_attr(not(test), no_std)]

// Panic handler
#[cfg(not(test))]
use panic_rtt_target as _;

use nrf52832_hal::{self as hal, pac};
use rtfm::app;
use rtt_target::{rprintln, rtt_init_print};
use rubble::{
    beacon::Beacon,
    link::{ad_structure::AdStructure, DeviceAddress, MIN_PDU_BUF},
};
use rubble_nrf5x::{
    radio::{BleRadio, PacketBuffer},
    utils::get_device_address,
};
use shtcx::{shtc1, ShtC1};

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

// BLE Beacon
const AD_STRUCTURE_MANUFACTURER_DATA: u8 = 0xff;

#[app(device = crate::pac, peripherals = true, monotonic = crate::monotonic_nrf52::Tim1)]
const APP: () = {
    struct Resources {
        // BLE
        #[init([0; MIN_PDU_BUF])]
        ble_tx_buf: PacketBuffer,
        #[init([0; MIN_PDU_BUF])]
        ble_rx_buf: PacketBuffer,
        radio: BleRadio,
        device_address: DeviceAddress,

        // Measurements
        sht: ShtC1<hal::twim::Twim<pac::TWIM0>>,
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

        // Initialize monotonic timer on TIMER1 (for RTFM)
        monotonic_nrf52::Tim1::initialize(TIMER1);

        // Initialize TWIM (I²C) peripheral
        let sda = gpio.p0_30.into_floating_input().degrade();
        let scl = gpio.p0_31.into_floating_input().degrade();
        let twim = hal::twim::Twim::new(
            TWIM0,
            hal::twim::Pins { sda, scl },
            hal::twim::Frequency::K250,
        );

        // Initialize SHT sensor
        let mut sht = shtc1(twim);
        rprintln!(
            "SHTC1: Device identifier is {}",
            sht.device_identifier().unwrap()
        );

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
            sht,
        }
    }

    /// Start a measurement
    #[task(resources = [sht, measurement_start], schedule = [collect_measurement])]
    fn start_measurement(ctx: start_measurement::Context) {
        let sht = ctx.resources.sht;
        let power_mode = shtcx::PowerMode::NormalMode;

        // Store the instant when this task was scheduled.
        // This ensures that there is no jitter in scheduling.
        *ctx.resources.measurement_start = Some(ctx.scheduled);

        // Trigger measurement
        sht.start_measurement(power_mode).unwrap();

        // Schedule measurement collection
        let timedelta = (shtcx::max_measurement_duration(sht, power_mode) as u32).micros();
        ctx.schedule
            .collect_measurement(Instant::now() + timedelta)
            .unwrap();
    }

    /// Collect a measurement. Then send the data using non-connectable BLE
    /// advertisement frames (beacons).
    #[task(
        resources = [sht, measurement_start, device_address, beacon],
        schedule = [start_measurement],
        spawn = [broadcast_beacon],
    )]
    fn collect_measurement(ctx: collect_measurement::Context) {
        static mut COUNTER: u16 = 0;

        // Take measurement start time
        let measurement_start = ctx
            .resources
            .measurement_start
            .take()
            .expect("Cannot collect measurement without starting a measurement first");

        // Collect measurement result from sensor
        let measurement = ctx.resources.sht.get_measurement_result().unwrap();
        rprintln!(
            "SHTC1 measurement: {}°C / {} %RH",
            measurement.temperature.as_degrees_celsius(),
            measurement.humidity.as_percent()
        );

        // Prepare beacon payload
        let temp = measurement
            .temperature
            .as_millidegrees_celsius()
            .to_le_bytes();
        let humi = measurement.humidity.as_millipercent().to_le_bytes();
        let counter_bytes = COUNTER.to_le_bytes();
        #[rustfmt::skip]
        let payload = [
            0xff, 0xff,
            counter_bytes[0], counter_bytes[1],
            SENSOR_TEMP, temp[0], temp[1], temp[2], temp[3], // i32 LE
            SENSOR_HUMI, humi[0], humi[1], humi[2], humi[3], // i32 LE
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
    #[task(resources = [radio, beacon], schedule = [broadcast_beacon])]
    fn broadcast_beacon(ctx: broadcast_beacon::Context, i: u8) {
        if i >= BEACON_BURST_COUNT {
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

    // Provide unused interrupts to RTFM for its scheduling
    extern "C" {
        fn SWI0_EGU0();
        fn SWI1_EGU1();
        fn SWI2_EGU2();
        fn SWI3_EGU3();
        fn SWI4_EGU4();
        fn SWI5_EGU5();
    }
};
