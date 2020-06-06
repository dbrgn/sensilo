/// A temperature measurement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Temperature(i32);

/// A humidity measurement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Humidity(i32);

impl Temperature {
    /// Create a new `Temperature` from little endian bytes.
    pub fn from_le_bytes(raw: [u8; 4]) -> Self {
        Self(i32::from_le_bytes(raw))
    }

    /// Return temperature in milli-degrees celsius.
    pub fn as_millidegrees_celsius(&self) -> i32 {
        self.0
    }

    /// Return temperature in degrees celsius.
    pub fn as_degrees_celsius(&self) -> f32 {
        self.0 as f32 / 1000.0
    }
}

impl Humidity {
    /// Create a new `Humidity` from little endian bytes.
    pub fn from_le_bytes(raw: [u8; 4]) -> Self {
        Self(i32::from_le_bytes(raw))
    }

    /// Return relative humidity in 1/1000 %RH.
    pub fn as_millipercent(&self) -> i32 {
        self.0
    }

    /// Return relative humidity in %RH.
    pub fn as_percent(&self) -> f32 {
        self.0 as f32 / 1000.0
    }
}

#[derive(Debug)]
pub struct Measurement<'a> {
    pub address: &'a [u8],
    pub rssi: u8,
    pub local_name: &'a str,
    pub temperature: Option<Temperature>,
    pub humidity: Option<Humidity>,
}

#[derive(Default)]
pub struct MeasurementBuilder<'a> {
    address: &'a [u8],
    rssi: u8,
    local_name: Option<&'a str>,
    temperature: Option<Temperature>,
    humidity: Option<Humidity>,
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

    pub fn temperature(&mut self, val: Temperature) -> &mut Self {
        self.temperature = Some(val);
        self
    }

    pub fn humidity(&mut self, val: Humidity) -> &mut Self {
        self.humidity = Some(val);
        self
    }

    pub fn build(self) -> Result<Measurement<'a>, &'static str> {
        Ok(Measurement {
            address: self.address,
            rssi: self.rssi,
            local_name: self.local_name.ok_or("Missing local name")?,
            temperature: self.temperature,
            humidity: self.humidity,
        })
    }
}
