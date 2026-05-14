//! Serde helper modules for base64-encoding binary fields when going over
//! a text transport (JSON HTTP API).
//!
//! Default `Vec<u8>` serialization in serde is a JSON array of integers,
//! which is both verbose and bandwidth-hostile for blobs like page
//! screenshots. The HTTP API layer (`lore-server`) and the future
//! `HttpBackend` in `lore-ui` both need a stable base64 representation;
//! keeping the helper here means a single source of truth.

/// `Option<Vec<u8>>` ↔ `Option<base64-encoded String>`.
///
/// Use as `#[serde(with = "crate::serde_b64::opt_vec")]` on the field.
pub mod opt_vec {
    use base64::Engine;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(bytes: &Option<Vec<u8>>, ser: S) -> Result<S::Ok, S::Error> {
        match bytes {
            Some(b) => {
                let encoded = base64::engine::general_purpose::STANDARD.encode(b);
                Some(encoded).serialize(ser)
            }
            None => Option::<String>::None.serialize(ser),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(de: D) -> Result<Option<Vec<u8>>, D::Error> {
        let opt = Option::<String>::deserialize(de)?;
        match opt {
            Some(s) => base64::engine::general_purpose::STANDARD
                .decode(s.as_bytes())
                .map(Some)
                .map_err(serde::de::Error::custom),
            None => Ok(None),
        }
    }
}

/// `Vec<u8>` ↔ base64-encoded `String`.
///
/// Use as `#[serde(with = "crate::serde_b64::vec")]` on the field.
pub mod vec {
    use base64::Engine;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8], ser: S) -> Result<S::Ok, S::Error> {
        let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
        encoded.serialize(ser)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(de: D) -> Result<Vec<u8>, D::Error> {
        let s = String::deserialize(de)?;
        base64::engine::general_purpose::STANDARD
            .decode(s.as_bytes())
            .map_err(serde::de::Error::custom)
    }
}
