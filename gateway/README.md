# Sensilo Gateway

Rust daemon that receives Sensilo advertisement frames (aka beacons) via
Bluetooth (through libpcap) and processes them.

Measurements are sent to an InfluxDB server.

## Setup

Ensure that the bluetooth adapter is powered on and in scan mode:

    bluetoothctl power on
    bluetoothctl scan on

Then run the daemon with the necessary permissions.

## Config

The daemon requires a config file called `config.toml`. Example:

```toml
[influxdb]
connection_string = "https://influxdb.example.com"
user = "influxuser"
pass = "influxpass"
db = "sensilo"

[[devices]]
name = "Sensilo1"
hex_addr = "864fe067997a"
location = "Kitchen"

[[devices]]
name = "Sensilo2"
hex_addr = "864fe067997b"
```

## Logging

To see the log output:

    export RUST_LOG=sensilo_gateway=debug
