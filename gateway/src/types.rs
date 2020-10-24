use std::fmt;

#[derive(Debug, PartialEq, Eq, Hash, Copy, Clone)]
pub struct Address(pub [u8; 6]);

impl Address {
    /// In the bluetooth protocol, the address bytes are inverted.
    ///
    /// If the slice passed in does not have length 6, panic.
    pub fn from_inverted_slice(addr: &[u8]) -> Self {
        assert_eq!(addr.len(), 6);
        Self([addr[5], addr[4], addr[3], addr[2], addr[1], addr[0]])
    }

    /// Parse the hex address.
    ///
    /// If the hex value is invalid, panic.
    pub fn from_hex(hexaddr: &str) -> Self {
        assert_eq!(hexaddr.len(), 12);
        let mut data = [0; 6];
        base16::decode_slice(hexaddr, &mut data).unwrap();
        Self(data)
    }
}

impl fmt::Display for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in &self.0 {
            write!(f, "{:x}", byte)?;
        }
        Ok(())
    }
}
