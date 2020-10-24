use serde::Deserialize;

#[derive(Deserialize)]
pub struct Config {
    pub devices: Vec<Device>,
}

#[derive(Deserialize)]
pub struct Device {
    pub name: String,
    pub hex_addr: String,
    pub location: Option<String>,
}
