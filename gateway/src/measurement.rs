#[derive(Debug)]
pub struct Measurement<'a> {
    address: &'a [u8],
    rssi: u8,
    local_name: &'a str,
    temperature: u32,
    humidity: u32,
}

#[derive(Default)]
pub struct MeasurementBuilder<'a> {
    address: &'a [u8],
    rssi: u8,
    local_name: Option<&'a str>,
    temperature: Option<u32>,
    humidity: Option<u32>,
}

impl<'a> MeasurementBuilder<'a> {
    pub fn new(address: &'a [u8], rssi: u8) -> Self {
        MeasurementBuilder {
            address,
            rssi,
            ..Default::default()
        }
    }

    pub fn local_name(&mut self, name: &'a str) -> &mut Self {
        self.local_name = Some(name);
        self
    }

    pub fn temperature(&mut self, val: u32) -> &mut Self {
        self.temperature = Some(val);
        self
    }

    pub fn humidity(&mut self, val: u32) -> &mut Self {
        self.humidity = Some(val);
        self
    }

    pub fn build(self) -> Result<Measurement<'a>, &'static str> {
        Ok(Measurement {
            address: self.address,
            rssi: self.rssi,
            local_name: self.local_name.ok_or("Missing local name")?,
            temperature: self.temperature.ok_or("Missing temperature")?,
            humidity: self.humidity.ok_or("Missing humidity")?,
        })
    }
}
