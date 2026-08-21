//! Frozen v0.8 tenant identity.
//!
//! `TenantId` is the filesystem-safe opaque tenant identifier: exactly 12
//! lowercase hexadecimal characters, CSPRNG-generated, independent of username
//! and of SQLite row ids.

use rand::{thread_rng, RngCore};
use std::fmt;
use std::str::FromStr;

/// Opaque tenant identifier: 12 lowercase hex characters (e.g. `fafafa12c3e4`).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct TenantId {
    hex: [u8; Self::LEN],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TenantIdError {
    InvalidLength { got: usize },
    InvalidCharacter { found: char },
}

impl fmt::Display for TenantIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLength { got } => {
                write!(
                    f,
                    "TenantId must be {} lowercase hex characters, got {got}",
                    TenantId::LEN
                )
            }
            Self::InvalidCharacter { found } => {
                write!(f, "TenantId must be lowercase hexadecimal, found {found:?}")
            }
        }
    }
}

impl std::error::Error for TenantIdError {}

impl TenantId {
    pub const LEN: usize = 12;

    /// Cryptographically secure random TenantId. Never derived from user data.
    pub fn generate() -> Self {
        let mut bytes = [0u8; 6];
        thread_rng().fill_bytes(&mut bytes);
        let encoded = hex::encode(bytes);
        Self::parse(&encoded).expect("CSPRNG hex encoding is 12 lowercase chars")
    }

    pub fn parse(s: &str) -> Result<Self, TenantIdError> {
        if s.len() != Self::LEN {
            return Err(TenantIdError::InvalidLength { got: s.len() });
        }
        if let Some(found) = s.chars().find(|c| !matches!(c, '0'..='9' | 'a'..='f')) {
            return Err(TenantIdError::InvalidCharacter { found });
        }
        let mut hex = [0u8; Self::LEN];
        hex.copy_from_slice(s.as_bytes());
        Ok(Self { hex })
    }

    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.hex).expect("TenantId is validated ASCII hex")
    }
}

impl fmt::Display for TenantId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl fmt::Debug for TenantId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("TenantId").field(&self.as_str()).finish()
    }
}

impl FromStr for TenantId {
    type Err = TenantIdError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

impl serde::Serialize for TenantId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> serde::Deserialize<'de> for TenantId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Self::parse(&s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_accepts_lowercase_12_hex() {
        let id = TenantId::parse("fafafa12c3e4").unwrap();
        assert_eq!(id.as_str(), "fafafa12c3e4");
        assert_eq!(id, TenantId::parse("fafafa12c3e4").unwrap());
    }

    #[test]
    fn parse_rejects_uppercase_and_wrong_length() {
        assert!(matches!(
            TenantId::parse("FAFAFA12C3E4"),
            Err(TenantIdError::InvalidCharacter { found: 'F' })
        ));
        assert!(matches!(
            TenantId::parse("abc"),
            Err(TenantIdError::InvalidLength { got: 3 })
        ));
        assert!(TenantId::parse("a1b2c3d4e5f67").is_err());
        assert!(TenantId::parse("../etc/passwd").is_err());
        assert!(TenantId::parse("gggggggggggg").is_err());
    }

    #[test]
    fn generate_is_opaque_lowercase_hex() {
        let a = TenantId::generate();
        let b = TenantId::generate();
        assert_ne!(a, b);
        assert_eq!(a.as_str().len(), 12);
        assert!(a
            .as_str()
            .chars()
            .all(|c| matches!(c, '0'..='9' | 'a'..='f')));
    }
}
