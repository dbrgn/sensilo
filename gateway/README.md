# Sensilo Gateway

Rust daemon that receives Sensilo advertisement frames (aka beacons) via
Bluetooth (through libpcap) and processes them.

## Setup

Ensure that the bluetooth adapter is powered on and in scan mode:

    bluetoothctl power on
    bluetoothctl scan on

Then run the daemon with the necessary permissions.

## Logging

To see the log output:

    export RUST_LOG=sensilo_gateway=debug
