use crate::StoreError;
use schomburg_core::{MetadataKey, MetadataValue};
use std::{
    collections::BTreeMap,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const TIMESTAMP_LENGTH: usize = 13;
const BEFORE_EPOCH: u8 = 0;
const AT_OR_AFTER_EPOCH: u8 = 1;

/// Encodes `SystemTime` into a sortable, fixed-width SQLite BLOB.
pub(crate) fn encode_timestamp(timestamp: SystemTime) -> [u8; TIMESTAMP_LENGTH] {
    let (marker, duration, invert) = match timestamp.duration_since(UNIX_EPOCH) {
        Ok(duration) => (AT_OR_AFTER_EPOCH, duration, false),
        Err(error) => (BEFORE_EPOCH, error.duration(), true),
    };

    let mut encoded = [0; TIMESTAMP_LENGTH];
    encoded[0] = marker;
    encoded[1..9].copy_from_slice(&duration.as_secs().to_be_bytes());
    encoded[9..].copy_from_slice(&duration.subsec_nanos().to_be_bytes());

    if invert {
        for byte in &mut encoded[1..] {
            *byte = !*byte;
        }
    }

    encoded
}

/// Decodes a timestamp BLOB and rejects malformed or unrepresentable values.
pub(crate) fn decode_timestamp(
    bytes: &[u8],
    field: &'static str,
) -> Result<SystemTime, StoreError> {
    if bytes.len() != TIMESTAMP_LENGTH {
        return Err(StoreError::MalformedStoredData {
            field,
            detail: format!("expected {TIMESTAMP_LENGTH} bytes, found {}", bytes.len()),
        });
    }

    let invert = match bytes[0] {
        BEFORE_EPOCH => true,
        AT_OR_AFTER_EPOCH => false,
        marker => {
            return Err(StoreError::UnsupportedStoredData {
                field,
                value: format!("timestamp marker {marker}"),
            });
        }
    };

    let mut seconds = [0; 8];
    let mut nanoseconds = [0; 4];
    seconds.copy_from_slice(&bytes[1..9]);
    nanoseconds.copy_from_slice(&bytes[9..]);
    if invert {
        for byte in &mut seconds {
            *byte = !*byte;
        }
        for byte in &mut nanoseconds {
            *byte = !*byte;
        }
    }

    let nanoseconds = u32::from_be_bytes(nanoseconds);
    if nanoseconds >= 1_000_000_000 {
        return Err(StoreError::MalformedStoredData {
            field,
            detail: format!("nanoseconds out of range: {nanoseconds}"),
        });
    }
    let duration = Duration::new(u64::from_be_bytes(seconds), nanoseconds);
    let timestamp = if invert {
        UNIX_EPOCH.checked_sub(duration)
    } else {
        UNIX_EPOCH.checked_add(duration)
    };

    timestamp.ok_or_else(|| StoreError::MalformedStoredData {
        field,
        detail: "timestamp is outside this platform's SystemTime range".to_owned(),
    })
}

pub(crate) fn encode_metadata(
    metadata: &BTreeMap<MetadataKey, MetadataValue>,
) -> Result<String, StoreError> {
    let values: BTreeMap<_, _> = metadata
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect();
    serde_json::to_string(&values).map_err(|error| StoreError::Serialization {
        detail: error.to_string(),
    })
}

pub(crate) fn decode_metadata(
    value: &str,
) -> Result<BTreeMap<MetadataKey, MetadataValue>, StoreError> {
    let values: BTreeMap<String, String> =
        serde_json::from_str(value).map_err(|error| StoreError::MalformedStoredData {
            field: "payload_metadata_json",
            detail: error.to_string(),
        })?;
    Ok(values
        .into_iter()
        .map(|(key, value)| (MetadataKey::new(key), MetadataValue::new(value)))
        .collect())
}
