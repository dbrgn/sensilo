# sensilo

A generic BLE sensor node based on the nRF52832. Firmware written in Rust with the RTIC framework.

Current status: PoC

## Development

### Flashing (cargo-embed)

Install cargo-embed:

    $ cargo install -f --git https://github.com/probe-rs/cargo-embed/

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

### Unlocking

When receiving a new board (e.g. the E73-TBB), the nRF often needs to be
unlocked before being able to connect to it with the debugger.

Instructions: https://blog.dbrgn.ch/2020/5/16/nrf52-unprotect-flash-jlink-openocd/
