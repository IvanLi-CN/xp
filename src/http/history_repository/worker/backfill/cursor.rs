use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Clone, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub(crate) struct HistoricalBackfillSortKey {
    pub(super) observed_at_unix_seconds: u64,
    pub(super) schema_id: String,
    #[serde(with = "backfill_cursor_key")]
    pub(super) record_key: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
pub(super) struct HistoricalBackfillPageCursor {
    pub(super) after: HistoricalBackfillSortKey,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) snapshot_end_unix_seconds: Option<u64>,
}

impl HistoricalBackfillSortKey {
    pub(crate) fn encode(&self) -> anyhow::Result<String> {
        Ok(URL_SAFE_NO_PAD.encode(serde_json::to_vec(self)?))
    }

    #[cfg(test)]
    pub(crate) fn decode(encoded: &str) -> anyhow::Result<Self> {
        if encoded.len() > 1_024 {
            anyhow::bail!("initial history backfill cursor exceeds limit");
        }
        Ok(serde_json::from_slice(&URL_SAFE_NO_PAD.decode(encoded)?)?)
    }
}

impl HistoricalBackfillPageCursor {
    pub(super) fn encode(&self) -> anyhow::Result<String> {
        Ok(URL_SAFE_NO_PAD.encode(serde_json::to_vec(self)?))
    }

    pub(super) fn decode(encoded: &str) -> anyhow::Result<Self> {
        if encoded.len() > 1_024 {
            anyhow::bail!("initial history backfill cursor exceeds limit");
        }
        let bytes = URL_SAFE_NO_PAD.decode(encoded)?;
        let value = serde_json::from_slice::<serde_json::Value>(&bytes)?;
        if value.get("after").is_some() {
            return Ok(serde_json::from_value(value)?);
        }
        Ok(Self {
            after: serde_json::from_value(value)?,
            snapshot_end_unix_seconds: None,
        })
    }
}

mod backfill_cursor_key {
    use super::*;

    pub(super) fn serialize<S>(value: &[u8], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&URL_SAFE_NO_PAD.encode(value))
    }

    pub(super) fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let encoded = String::deserialize(deserializer)?;
        URL_SAFE_NO_PAD
            .decode(encoded)
            .map_err(serde::de::Error::custom)
    }
}
