use crate::types::Address;

/// A temperature measurement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Temperature(i32);

/// A humidity measurement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Humidity(i32);

/// An ambient light measurement.
#[derive(Debug, Clone, PartialEq)]
pub struct AmbientLight(f32);

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

impl AmbientLight {
    /// Create a new `AmbientLight` from little endian bytes.
    pub fn from_le_bytes(raw: [u8; 4]) -> Self {
        Self(f32::from_le_bytes(raw))
    }

    /// Return ambient light in lux.
    pub fn as_lux(&self) -> f32 {
        self.0
    }
}

#[derive(Debug)]
pub struct Measurement<'a> {
    pub address: Address,
    pub rssi: u8,
    pub local_name: &'a str,
    pub counter: u16,
    pub temperature: Option<Temperature>,
    pub humidity: Option<Humidity>,
    pub ambient_light: Option<AmbientLight>,
}

pub struct MeasurementBuilder<'a> {
    address: Address,
    rssi: u8,
    local_name: Option<&'a str>,
    counter: Option<u16>,
    temperature: Option<Temperature>,
    humidity: Option<Humidity>,
    ambient_light: Option<AmbientLight>,
    parse_error: bool,
}

impl<'a> MeasurementBuilder<'a> {
    pub fn new(address: Address, rssi: u8) -> Self {
        MeasurementBuilder {
            address,
            rssi,
            local_name: None,
            counter: None,
            temperature: None,
            humidity: None,
            ambient_light: None,
            parse_error: false,
        }
    }

    pub fn local_name(&mut self, name: &'a str) -> &mut Self {
        self.local_name = Some(name);
        self
    }

    pub fn counter(&mut self, counter: u16) -> &mut Self {
        self.counter = Some(counter);
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

    pub fn ambient_light(&mut self, val: AmbientLight) -> &mut Self {
        self.ambient_light = Some(val);
        self
    }

    pub fn parse_payload(&mut self, payload: &[u8]) -> Result<&mut Self, &'static str> {
        let mut bytes = payload.iter();

        macro_rules! consume {
            ($name:expr, $count:expr) => {{
                let mut data = [0; $count];
                for i in 0..$count {
                    data[i] = *bytes.next().ok_or_else(|| {
                        log::warn!("Malformed payload: Missing {}", $name);
                        self.parse_error = true;
                        "Malformed payload"
                    })?;
                }
                data
            }};
        }

        // Parse counter
        let counter = consume!("counter", 2);
        self.counter(u16::from_le_bytes(counter));

        // Parse data
        while let Some(payload_type) = bytes.next() {
            match payload_type {
                0x01 => {
                    let raw = consume!("temperature", 4);
                    self.temperature(Temperature::from_le_bytes(raw));
                }
                0x02 => {
                    let raw = consume!("humidity", 4);
                    self.humidity(Humidity::from_le_bytes(raw));
                }
                0x04 => {
                    let raw = consume!("ambient light", 4);
                    self.ambient_light(AmbientLight::from_le_bytes(raw));
                }
                other => {
                    log::info!("Unknown payload type: {}", other);
                }
            }
        }

        Ok(self)
    }

    pub fn build(self) -> Result<Measurement<'a>, &'static str> {
        if self.parse_error {
            return Err("Error while parsing packet");
        }
        Ok(Measurement {
            address: self.address,
            rssi: self.rssi,
            local_name: self.local_name.ok_or("Missing local name")?,
            counter: self.counter.ok_or("Missing counter")?,
            temperature: self.temperature,
            humidity: self.humidity,
            ambient_light: self.ambient_light,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_payload() {
        #[rustfmt::skip]
        let payload = [
            // 2 byte counter
            52, 4,
            // Payload type 1: Temperature
            1, 250, 98, 0, 0,
            // Payload type 2: Humidity
            2, 230, 192, 0, 0,
            // Payload type 4: Ambient light
            4, 80, 252, 152, 66,
        ];
        let address = [1, 2, 3, 4, 5, 6, 7, 8];
        let mut builder = MeasurementBuilder::new(&address, 123);
        builder.local_name("Sensilo");
        builder.parse_payload(&payload).unwrap();
        let measurement = builder.build().unwrap();
        assert_eq!(measurement.address, &address);
        assert_eq!(measurement.rssi, 123);
        assert_eq!(measurement.local_name, "Sensilo");
        assert_eq!(measurement.counter, 1076);
        assert_eq!(measurement.temperature, Some(Temperature(25_338)));
        assert_eq!(measurement.humidity, Some(Humidity(49_382)));
        assert_eq!(measurement.ambient_light, Some(AmbientLight(76.4928)));
    }
}
