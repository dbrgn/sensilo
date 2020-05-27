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
use shtcx::{shtc1, ShtC1, Measurement};

mod delay;
mod monotonic_nrf52;

use monotonic_nrf52::U32Ext;

const MEASURE_INTERVAL_MS: u32 = 210; // Should be divisible by 3

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
    #[task(resources = [sht], schedule = [collect_measurement])]
    fn start_measurement(ctx: start_measurement::Context) {
        ctx.resources.sht.start_measurement(shtcx::PowerMode::NormalMode).unwrap();

        // Schedule measurement collection
        ctx.schedule
            .collect_measurement(ctx.scheduled + (MEASURE_INTERVAL_MS / 3).millis())
            .unwrap();
    }

    /// Collect a measurement
    #[task(resources = [sht], schedule = [broadcast_beacon])]
    fn collect_measurement(ctx: collect_measurement::Context) {
        let measurement = ctx.resources.sht.get_measurement_result().unwrap();
        rprintln!(
            "SHTC1 measurement: {}°C / {} %RH",
            measurement.temperature.as_degrees_celsius(),
            measurement.humidity.as_percent()
        );

        // Schedule beacon transmission
        ctx.schedule
            .broadcast_beacon(ctx.scheduled + (MEASURE_INTERVAL_MS / 3).millis(), measurement)
            .unwrap();
    }

    /// Broadcast the beacon exactly once.
    #[task(resources = [radio, device_address], schedule = [start_measurement])]
    fn broadcast_beacon(ctx: broadcast_beacon::Context, measurement: Measurement) {
        // Beacon payload
        let temp = measurement.temperature.as_millidegrees_celsius().to_le_bytes();
        let humi = measurement.humidity.as_millipercent().to_le_bytes();

        // Create beacon
        let beacon = Beacon::new(
            *ctx.resources.device_address,
            &[
                AdStructure::CompleteLocalName("Sensilo"),
                AdStructure::ServiceData16 {
                    uuid: 0x181a,
                    data: &temp,
                },
                AdStructure::ServiceData16 {
                    uuid: 0x181a,
                    data: &humi,
                },
            ],
        )
        .expect("Could not create beacon");

        // Broadcast beacon
        beacon.broadcast(ctx.resources.radio);

        // Schedule a new measurement
        ctx.schedule
            .start_measurement(ctx.scheduled + (MEASURE_INTERVAL_MS / 3).millis())
            .unwrap();

        rprintln!("Sent beacon");
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
