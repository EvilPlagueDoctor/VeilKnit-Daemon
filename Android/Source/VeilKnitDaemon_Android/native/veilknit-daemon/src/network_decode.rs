//! Bounded decoding helpers for data received from DHT records or app messages.
//!
//! PATCH A: keep all hostile-input size limits in one place so individual
//! modules do not accidentally fall back to unrestricted deserialization.

use bincode::Options;
use serde::de::DeserializeOwned;

/// Conservative upper bound for a single structured DHT value.
/// Veilid records used by this project are expected to be far smaller.
pub const MAX_NETWORK_DHT_VALUE_BYTES: usize = 64 * 1024;

/// Route blobs can be larger than ordinary metadata but should still have a
/// firm protocol limit before allocation-heavy decoding is attempted.
pub const MAX_ROUTE_BLOB_RECORD_BYTES: usize = 128 * 1024;

#[derive(Debug)]
pub enum NetworkDecodeError {
    EnvelopeTooLarge { actual: usize, maximum: usize },
    Decode(String),
}

impl std::fmt::Display for NetworkDecodeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EnvelopeTooLarge { actual, maximum } => write!(
                formatter,
                "network envelope is {actual} bytes; maximum is {maximum} bytes"
            ),
            Self::Decode(error) => formatter.write_str(error),
        }
    }
}

impl std::error::Error for NetworkDecodeError {}

pub fn decode_bincode_limited<T: DeserializeOwned>(
    bytes: &[u8],
    maximum: usize,
) -> Result<T, NetworkDecodeError> {
    if bytes.len() > maximum {
        return Err(NetworkDecodeError::EnvelopeTooLarge {
            actual: bytes.len(),
            maximum,
        });
    }

    bincode::DefaultOptions::new()
        // Match bincode::{serialize, deserialize} compatibility in bincode 1.x.
        .with_fixint_encoding()
        .reject_trailing_bytes()
        .with_limit(maximum as u64)
        .deserialize(bytes)
        .map_err(|error| NetworkDecodeError::Decode(error.to_string()))
}

pub fn decode_json_limited<T: DeserializeOwned>(
    bytes: &[u8],
    maximum: usize,
) -> Result<T, NetworkDecodeError> {
    if bytes.len() > maximum {
        return Err(NetworkDecodeError::EnvelopeTooLarge {
            actual: bytes.len(),
            maximum,
        });
    }

    serde_json::from_slice(bytes)
        .map_err(|error| NetworkDecodeError::Decode(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Example {
        value: u32,
        label: String,
    }

    #[test]
    fn bounded_bincode_round_trip_uses_legacy_encoding() {
        let expected = Example {
            value: 42,
            label: "bounded".to_string(),
        };
        let bytes = bincode::serialize(&expected).expect("serialize test value");
        let decoded: Example =
            decode_bincode_limited(&bytes, 1024).expect("decode test value");
        assert_eq!(decoded, expected);
    }

    #[test]
    fn oversized_envelope_is_rejected_before_decode() {
        let bytes = vec![0u8; 17];
        let error = decode_bincode_limited::<Vec<u8>>(&bytes, 16)
            .expect_err("oversized value must be rejected");
        assert!(matches!(
            error,
            NetworkDecodeError::EnvelopeTooLarge {
                actual: 17,
                maximum: 16
            }
        ));
    }

    #[test]
    fn trailing_bincode_bytes_are_rejected() {
        let mut bytes = bincode::serialize(&7u32).expect("serialize integer");
        bytes.push(0);
        assert!(decode_bincode_limited::<u32>(&bytes, 64).is_err());
    }
}
