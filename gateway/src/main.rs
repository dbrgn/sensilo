use std::collections::HashMap;

use futures::StreamExt;
use hci::protocol::{
    BasicDataType_Data, HciEvent_Event, HciMessage, HciMessage_Message, LeMetaEvent_Event,
};
use lru::LruCache;
use pcap_async::{Config, Handle, Packet, PacketStream};

mod config;
mod measurement;
mod types;

use measurement::MeasurementBuilder;
use types::Address;

// TODO: Parse from file
const ADDRESSES: [Address; 2] = [
    // Devboard
    Address([79, 67, 92, 159, 209, 114]),
    // Sensilo 1
    Address([0x73, 0x4f, 0x74, 0x67, 0xf1, 0x7a]),
];

// Store a LRU cache with the last `DEDUPLICATION_LRU_SIZE` counters for every address.
// If a counter value is contained in the cache, ignore the message.
const DEDUPLICATION_LRU_SIZE: usize = 5;
type DeduplicationCache = HashMap<Address, LruCache<u16, ()>>;

fn main() -> std::io::Result<()> {
    env_logger::init();

    println!("Sensilo Gateway\n");

    // Parse config
    println!("Loading config.toml...");
    let config: config::Config = toml::from_str(&std::fs::read_to_string("config.toml")?)?;

    println!("Available bluetooth capture interfaces:");
    for iface in pcap_async::Info::all().expect("Could not get list of interfaces") {
        if iface.name.contains("blue") || iface.name.contains("ble") {
            println!("  {}", iface.name);
            for ip in iface.ips {
                println!("    {}", ip);
            }
        }
    }
    println!();

    smol::run(async {
        println!("Opening device bluetooth0...");
        let handle = Handle::live_capture("bluetooth0").expect("No handle created");
        //let handle = Handle::file_capture("/tmp/ble.pcap").expect("No handle created");

        let mut config = Config::default();
        config.with_blocking(true);

        let mut stream =
            PacketStream::new(config, std::sync::Arc::clone(&handle)).expect("Failed to build");

        let mut deduplication_cache: DeduplicationCache = HashMap::new();
        while let Some(packets_result) = stream.next().await {
            if let Ok(packets) = packets_result {
                for packet in packets {
                    log::trace!("{:?}", packet);
                    let _ = process_packet(packet, &mut deduplication_cache);
                }
            } else {
                println!("Error: {:?}", packets_result);
            }
        }

        Ok(())
    })
}

fn process_packet(packet: Packet, deduplication_cache: &mut DeduplicationCache) -> Option<()> {
    // Validate length
    if packet.original_length() != packet.actual_length() {
        log::debug!(
            "Invalid packet length: {} != {}",
            packet.original_length(),
            packet.actual_length()
        );
        return None;
    }

    // Try to parse HCI message
    let payload = &packet.data()[4..];
    let parsed = HciMessage::parse(payload)
        .map_err(|e| {
            log::debug!("Could not parse HCI message");
            e
        })
        .ok()?;

    if !parsed.0.is_empty() {
        log::debug!("Payload parsed incompletely");
        return None;
    }

    // Extract event
    let event = if let HciMessage_Message::HciEvent(val) = parsed.1.get_message() {
        val
    } else {
        log::debug!("Ignoring non-event message");
        return None;
    };

    // We're only interested in LE meta events
    let le_event = if let HciEvent_Event::LeMetaEvent(val) = event.get_event() {
        val
    } else {
        log::debug!("Ignoring non-LeMetaEvent event");
        return None;
    };

    // We're only interested in advertising reports
    let adv_report = if let LeMetaEvent_Event::LeAdvertisingReport(val) = le_event.get_event() {
        val
    } else {
        log::debug!("Ignoring non-LeAdvertisingReport");
        return None;
    };

    // Filter by address
    let address = Address::from_inverted_slice(&adv_report.get_address());
    if !ADDRESSES.contains(&address) {
        log::debug!("Ignoring device with address {}", address);
        return None;
    }

    // Get data
    let mut builder = MeasurementBuilder::new(address, adv_report.get_rssi());
    log::trace!("Frame: {:?}", adv_report);
    for datum in adv_report.get_data() {
        match datum.get_data() {
            BasicDataType_Data::CompleteLocalName(name) => {
                builder.local_name(name.get_local_name());
            }
            BasicDataType_Data::ManufacturerSpecificData(data) => {
                if data.get_company_identifier_code() == 0xffff {
                    let payload = data.get_data();
                    log::trace!("Payload: {:?}", payload);
                    if let Err(e) = builder.parse_payload(&payload) {
                        log::warn!("Could not parse payload: {}", e);
                    }
                } else {
                    // Not a Sensilo advertisement frame
                }
            }
            other => {
                log::debug!("Ignoring datum in advertising report: {:?}", other);
            }
        }
    }
    let measurement = builder.build().unwrap();

    // Deduplicate beacons
    let lru = deduplication_cache
        .entry(address)
        .or_insert_with(|| LruCache::new(DEDUPLICATION_LRU_SIZE));
    if lru.get(&measurement.counter).is_some() {
        log::debug!("Ignoring duplicate frame (counter {})", measurement.counter);
        return None;
    } else {
        lru.put(measurement.counter, ());
    }

    println!(
        "{} ({} RSSI): [{}] {} °C | {} %RH | {} Lux",
        measurement.local_name,
        measurement.rssi,
        measurement.counter,
        measurement
            .temperature
            .map(|t| t.as_degrees_celsius())
            .unwrap_or(-1.0),
        measurement.humidity.map(|h| h.as_percent()).unwrap_or(-1.0),
        measurement
            .ambient_light
            .map(|h| h.as_lux())
            .unwrap_or(-1.0),
    );

    Some(())
}
