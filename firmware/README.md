# Sensilo Node Firmware

Firmware uses the RTIC embedded concurrency framework.

## Tasks

The following tasks make up a measurement cycle:

    +----+    +-----------------+        +-------------------+
    |init| -> |start_measurement| -(t)-> |collect_measurement|
    +----+    +-----------------+        +-------------------+
                                                      |
                                                      v
                                        +----------------+
                                    +-- |broadcast_beacon|
                                    |   +----------------+
                                    |        ^    |
                                    |        |    |
                                    +--(b)---+    |
                                                  v
                                          +----------+
                                          |power_down|
                                          +----------+
Delays:

- `t`: Max measurement duration for the sensor used
- `b`: BEACON_BURST_INTERVAL_MS

## Packet Format

Data is broadcasted in an unconnectable BLE advertisement frame (aka beacon).
The advertising data contains the device name ("Sensilo") as well as a
manufacturer specific entry (Company ID `0xff`) with the actual payload.

The payload starts with a 16 bit counter (starting at 0, incremented for every
beacon burst), followed by measurement entries.

Every measurement entry starts with a single-byte type flag (e.g. `0x01` for a
temperature measurement) followed by a type-specific payload.

All multi-byte values are in little endian byte order.

## Measurement Types

| Type | Description | Value Encoding |
|------|-------------|----------------|
| 0x01 | Temperature | Millidegrees Celsius (i32) |
| 0x02 | Relative Humidity | Millipercent (i32) |
| 0x03 | Particulate Matter | TBD |

## Development

### Unlocking

When receiving a new board (e.g. the E73-TBB), the nRF often needs to be
unlocked before being able to connect to it with the debugger.

Instructions: https://blog.dbrgn.ch/2020/5/16/nrf52-unprotect-flash-jlink-openocd/

### Flashing (cargo-embed)

Install cargo-embed:

    $ cargo install -f cargo-embed

Flash the target:

    $ cargo embed --release

### Flashing (openocd)

Run OpenOCD:

    $ ./openocd.sh

Run the code

    $ cargo run [--release]

### Flashing (J-Link GDB Server)

Run JLinkGDBServer:

    $ ./jlinkgdbserver.sh

Run the code

    $ cargo run [--release]
