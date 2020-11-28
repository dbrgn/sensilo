use serde::Deserialize;

#[derive(Deserialize, Debug)]
pub struct Config {
    pub devices: Vec<Device>,
    pub influxdb: InfluxDb,
}

#[derive(Deserialize, Debug)]
pub struct Device {
    pub name: String,
    pub hex_addr: String,
    pub location: Option<String>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct InfluxDb {
    pub connection_string: String,
    pub user: String,
    pub pass: String,
    pub db: String,
}
