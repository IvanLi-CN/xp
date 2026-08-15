// These contracts are intentionally introduced before their Raft action consumers arrive.
#![allow(dead_code)]

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};

const PUBLIC_KEY_BYTES: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RepositoryIdentityError {
    InvalidNodeId {
        reason: &'static str,
    },
    InvalidPublicKey {
        kind: &'static str,
        reason: &'static str,
    },
}

impl std::fmt::Display for RepositoryIdentityError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidNodeId { reason } => {
                write!(formatter, "invalid repository node id: {reason}")
            }
            Self::InvalidPublicKey { kind, reason } => {
                write!(formatter, "invalid {kind} public key: {reason}")
            }
        }
    }
}

impl std::error::Error for RepositoryIdentityError {}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct RepositoryNodeId(String);

impl RepositoryNodeId {
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for RepositoryNodeId {
    type Error = RepositoryIdentityError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.trim().is_empty() {
            return Err(RepositoryIdentityError::InvalidNodeId {
                reason: "must not be empty",
            });
        }
        if value != value.trim() {
            return Err(RepositoryIdentityError::InvalidNodeId {
                reason: "must not have surrounding whitespace",
            });
        }
        if value.chars().any(char::is_control) {
            return Err(RepositoryIdentityError::InvalidNodeId {
                reason: "must not contain control characters",
            });
        }

        Ok(Self(value))
    }
}

impl Serialize for RepositoryNodeId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for RepositoryNodeId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::try_from(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Ed25519PublicKey([u8; PUBLIC_KEY_BYTES]);

impl Ed25519PublicKey {
    pub(crate) fn from_bytes(
        bytes: [u8; PUBLIC_KEY_BYTES],
    ) -> Result<Self, RepositoryIdentityError> {
        validate_public_key_bytes("ed25519", &bytes)?;
        Ok(Self(bytes))
    }

    pub(crate) fn as_bytes(&self) -> &[u8; PUBLIC_KEY_BYTES] {
        &self.0
    }
}

impl Serialize for Ed25519PublicKey {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&URL_SAFE_NO_PAD.encode(self.0))
    }
}

impl<'de> Deserialize<'de> for Ed25519PublicKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        decode_public_key("ed25519", String::deserialize(deserializer)?)
            .map(Self)
            .map_err(D::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct X25519PublicKey([u8; PUBLIC_KEY_BYTES]);

impl X25519PublicKey {
    pub(crate) fn from_bytes(
        bytes: [u8; PUBLIC_KEY_BYTES],
    ) -> Result<Self, RepositoryIdentityError> {
        validate_public_key_bytes("x25519 relay", &bytes)?;
        Ok(Self(bytes))
    }

    pub(crate) fn as_bytes(&self) -> &[u8; PUBLIC_KEY_BYTES] {
        &self.0
    }
}

impl Serialize for X25519PublicKey {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&URL_SAFE_NO_PAD.encode(self.0))
    }
}

impl<'de> Deserialize<'de> for X25519PublicKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        decode_public_key("x25519 relay", String::deserialize(deserializer)?)
            .map(Self)
            .map_err(D::Error::custom)
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct RepositoryNodeIdentity {
    node_id: RepositoryNodeId,
    ed25519_public_key: Ed25519PublicKey,
    x25519_relay_public_key: X25519PublicKey,
}

impl RepositoryNodeIdentity {
    pub(crate) fn new(
        node_id: RepositoryNodeId,
        ed25519_public_key: Ed25519PublicKey,
        x25519_relay_public_key: X25519PublicKey,
    ) -> Result<Self, RepositoryIdentityError> {
        Ok(Self {
            node_id,
            ed25519_public_key,
            x25519_relay_public_key,
        })
    }

    pub(crate) fn node_id(&self) -> &RepositoryNodeId {
        &self.node_id
    }

    pub(crate) fn ed25519_public_key(&self) -> &Ed25519PublicKey {
        &self.ed25519_public_key
    }

    pub(crate) fn x25519_relay_public_key(&self) -> &X25519PublicKey {
        &self.x25519_relay_public_key
    }
}

impl<'de> Deserialize<'de> for RepositoryNodeIdentity {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawIdentity {
            node_id: RepositoryNodeId,
            ed25519_public_key: Ed25519PublicKey,
            x25519_relay_public_key: X25519PublicKey,
        }

        let raw = RawIdentity::deserialize(deserializer)?;
        Self::new(
            raw.node_id,
            raw.ed25519_public_key,
            raw.x25519_relay_public_key,
        )
        .map_err(D::Error::custom)
    }
}

fn decode_public_key(
    kind: &'static str,
    encoded: String,
) -> Result<[u8; PUBLIC_KEY_BYTES], RepositoryIdentityError> {
    let bytes =
        URL_SAFE_NO_PAD
            .decode(encoded)
            .map_err(|_| RepositoryIdentityError::InvalidPublicKey {
                kind,
                reason: "must be unpadded base64url",
            })?;
    let bytes: [u8; PUBLIC_KEY_BYTES] =
        bytes
            .try_into()
            .map_err(|bytes: Vec<u8>| RepositoryIdentityError::InvalidPublicKey {
                kind,
                reason: if bytes.len() < PUBLIC_KEY_BYTES {
                    "must contain 32 bytes"
                } else {
                    "must contain exactly 32 bytes"
                },
            })?;
    validate_public_key_bytes(kind, &bytes)?;
    Ok(bytes)
}

fn validate_public_key_bytes(
    kind: &'static str,
    bytes: &[u8; PUBLIC_KEY_BYTES],
) -> Result<(), RepositoryIdentityError> {
    if bytes.iter().all(|byte| *byte == 0) {
        return Err(RepositoryIdentityError::InvalidPublicKey {
            kind,
            reason: "must not be all zeroes",
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests;
