use serde::Deserialize;

#[derive(Deserialize, Debug)]
pub struct Config {
    pub devices: Vec<Device>,
}

#[derive(Deserialize, Debug)]
pub struct Device {
    pub name: String,
    pub hex_addr: String,
    pub location: Option<String>,
}
