use futures::StreamExt;
use hci::protocol::{
    BasicDataType_Data, HciEvent_Event, HciMessage, HciMessage_Message, LeMetaEvent_Event,
};
use pcap_async::{Config, Handle, Packet, PacketStream};

mod measurement;

use measurement::{Humidity, MeasurementBuilder, Temperature};

const ADDRESSES: [&[u8]; 1] = [
    // Sensilo 1
    &[79, 67, 92, 159, 209, 114],
];

fn main() -> std::io::Result<()> {
    env_logger::init();

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
        //let handle = Handle::live_capture("bluetooth0").expect("No handle created");
        let handle = Handle::file_capture("/tmp/ble.pcap").expect("No handle created");
        let mut stream = PacketStream::new(Config::default(), std::sync::Arc::clone(&handle))
            .expect("Failed to build");

        while let Some(packets_result) = stream.next().await {
            if let Ok(packets) = packets_result {
                for packet in packets {
                    let _ = process_packet(packet);
                }
            } else {
                println!("Error: {:?}", packets_result);
            }
        }

        Ok(())
    })
}

fn process_packet(packet: Packet) -> Option<()> {
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
    if !ADDRESSES.contains(&adv_report.get_address()) {
        return None;
    }

    // Get data
    let mut builder = MeasurementBuilder::new(adv_report.get_address(), adv_report.get_rssi());
    log::trace!("Frame: {:?}", adv_report);
    for datum in adv_report.get_data() {
        match datum.get_data() {
            BasicDataType_Data::CompleteLocalName(name) => {
                builder.local_name(name.get_local_name());
            }
            BasicDataType_Data::ManufacturerSpecificData(data) => {
                if data.get_company_identifier_code() == 0xffff {
                    let payload = data.get_data();
                    match payload.get(0) {
                        Some(0x01) => {
                            // Temperature
                            if payload.len() != 5 {
                                log::warn!(
                                    "Invalid temperature packet: Length is {}, not 5",
                                    payload.len()
                                );
                                continue;
                            }
                            builder.temperature(Temperature::from_le_bytes([
                                payload[1], payload[2], payload[3], payload[4],
                            ]));
                        }
                        Some(0x02) => {
                            // Humidity
                            if payload.len() != 5 {
                                log::warn!(
                                    "Invalid humidity packet: Length is {}, not 5",
                                    payload.len()
                                );
                                continue;
                            }
                            builder.humidity(Humidity::from_le_bytes([
                                payload[1], payload[2], payload[3], payload[4],
                            ]));
                        }
                        Some(other) => {
                            log::info!("Unknown payload type: {}", other);
                        }
                        None => {
                            log::info!("Empty payload");
                        }
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
    println!(
        "{} ({} RSSI): {:?} / °C {:?} %RH",
        measurement.local_name,
        measurement.rssi,
        measurement.temperature.map(|t| t.as_degrees_celsius()).unwrap_or(-1.0),
        measurement.humidity.map(|h| h.as_percent()).unwrap_or(-1.0),
    );

    Some(())
}
