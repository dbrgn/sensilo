#![no_main]
#![cfg_attr(not(test), no_std)]

// Panic handler
#[cfg(not(test))]
use panic_rtt_target as _;

use nrf52832_hal::{self as hal, pac};
use rtfm::app;
use rtt_target::{rprintln, rtt_init_print};
use rubble::{
    config::Config,
    gatt::BatteryServiceAttrs,
    l2cap::{BleChannelMap, L2CAPState},
    link::ad_structure::AdStructure,
    link::queue::{PacketQueue, SimpleQueue},
    link::{LinkLayer, Responder, MIN_PDU_BUF},
    security::NoSecurity,
    time::{Duration as RubbleDuration, Timer},
};
use rubble_nrf5x::{
    radio::{BleRadio, PacketBuffer},
    timer::BleTimer,
    utils::get_device_address,
};
use shtcx::{shtc1, ShtCx};

mod delay;

pub struct AppConfig {}

impl Config for AppConfig {
    type Timer = BleTimer<hal::target::TIMER2>;
    type Transmitter = BleRadio;
    type ChannelMapper = BleChannelMap<BatteryServiceAttrs, NoSecurity>;
    type PacketQueue = &'static mut SimpleQueue;
}

#[app(device = crate::pac, peripherals = true)]
const APP: () = {
    struct Resources {
        // BLE
        #[init([0; MIN_PDU_BUF])]
        ble_tx_buf: PacketBuffer,
        #[init([0; MIN_PDU_BUF])]
        ble_rx_buf: PacketBuffer,
        #[init(SimpleQueue::new())]
        tx_queue: SimpleQueue,
        #[init(SimpleQueue::new())]
        rx_queue: SimpleQueue,
        radio: BleRadio,
        ble_ll: LinkLayer<AppConfig>,
        ble_r: Responder<AppConfig>,
    }

    #[init(resources = [ble_tx_buf, ble_rx_buf, tx_queue, rx_queue])]
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
            TIMER0,
            TIMER2,
            TWIM0,
            ..
        } = ctx.device;

        // Set up clocks. On reset, the high frequency clock is already used,
        // but we also need to switch to the external HF oscillator. This is
        // needed for Bluetooth to work.
        let _clocks = hal::clocks::Clocks::new(CLOCK).enable_ext_hfosc();

        // Set up GPIO peripheral
        let gpio = hal::gpio::p0::Parts::new(P0);

        // Set up delay provider on TIMER0
        let delay = delay::TimerDelay::new(TIMER0);

        // Initialize TWIM (I²C) peripheral
        let sda = gpio.p0_30.into_floating_input().degrade();
        let scl = gpio.p0_31.into_floating_input().degrade();
        let twim = hal::twim::Twim::new(
            TWIM0,
            hal::twim::Pins { sda, scl },
            hal::twim::Frequency::K250,
        );
        let mut sht = shtc1(twim, delay);
        rprintln!(
            "SHTC1: Device identifier is {}",
            sht.device_identifier().unwrap()
        );
        let measurement = sht.measure(shtcx::PowerMode::NormalMode).unwrap();
        rprintln!(
            "SHTC1: {}°C / {} %RH",
            measurement.temperature.as_degrees_celsius(),
            measurement.humidity.as_percent()
        );

        // Initialize BLE timer on TIMER2
        let ble_timer = BleTimer::init(TIMER2);

        // Get bluetooth device address
        let device_address = get_device_address();
        rprintln!("Bluetooth device address: {:?}", device_address);

        // Initialize radio
        let mut radio = BleRadio::new(
            RADIO,
            &FICR,
            ctx.resources.ble_tx_buf,
            ctx.resources.ble_rx_buf,
        );

        // Create bluetooth TX/RX queues
        let (tx, tx_cons) = ctx.resources.tx_queue.split();
        let (rx_prod, rx) = ctx.resources.rx_queue.split();

        // Create the actual BLE stack objects
        let mut ble_ll = LinkLayer::<AppConfig>::new(device_address, ble_timer);
        let ble_r = Responder::<AppConfig>::new(
            tx,
            rx,
            L2CAPState::new(BleChannelMap::with_attributes(BatteryServiceAttrs::new())),
        );

        // Send advertisement and set up regular interrupt
        let next_update = ble_ll
            .start_advertise(
                RubbleDuration::from_millis(200),
                &[AdStructure::CompleteLocalName("Sensilo")],
                &mut radio,
                tx_cons,
                rx_prod,
            )
            .unwrap();
        ble_ll.timer().configure_interrupt(next_update);

        rprintln!("Init done");
        init::LateResources {
            radio,
            ble_ll,
            ble_r,
        }
    }

    /// Hook up the RADIO interrupt to the Rubble BLE stack.
    #[task(binds = RADIO, resources = [radio, ble_ll], spawn = [ble_worker], priority = 3)]
    fn radio(cx: radio::Context) {
        let ble_ll: &mut LinkLayer<AppConfig> = cx.resources.ble_ll;
        if let Some(cmd) = cx
            .resources
            .radio
            .recv_interrupt(ble_ll.timer().now(), ble_ll)
        {
            cx.resources.radio.configure_receiver(cmd.radio);
            ble_ll.timer().configure_interrupt(cmd.next_update);

            if cmd.queued_work {
                // If there's any lower-priority work to be done, ensure that happens.
                // If we fail to spawn the task, it's already scheduled.
                cx.spawn.ble_worker().ok();
            }
        }
    }

    /// Hook up the TIMER2 interrupt to the Rubble BLE stack.
    #[task(binds = TIMER2, resources = [radio, ble_ll], spawn = [ble_worker], priority = 3)]
    fn timer2(cx: timer2::Context) {
        let timer = cx.resources.ble_ll.timer();
        if !timer.is_interrupt_pending() {
            return;
        }
        timer.clear_interrupt();

        let cmd = cx.resources.ble_ll.update_timer(&mut *cx.resources.radio);
        cx.resources.radio.configure_receiver(cmd.radio);

        cx.resources
            .ble_ll
            .timer()
            .configure_interrupt(cmd.next_update);

        if cmd.queued_work {
            // If there's any lower-priority work to be done, ensure that happens.
            // If we fail to spawn the task, it's already scheduled.
            cx.spawn.ble_worker().ok();
        }
    }

    /// Lower-priority task spawned from RADIO and TIMER2 interrupts.
    #[task(resources = [ble_r], priority = 2)]
    fn ble_worker(cx: ble_worker::Context) {
        // Fully drain the packet queue
        while cx.resources.ble_r.has_work() {
            cx.resources.ble_r.process_one().unwrap();
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
