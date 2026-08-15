use serde::{Deserialize, Deserializer, Serialize, de::Error as _};

pub(crate) const DEFAULT_HISTORY_REPOSITORY_QUOTA_BYTES: u64 = 10 * 1024 * 1024 * 1024;
pub(crate) const HISTORY_REPOSITORY_LOW_SPACE_GUARD_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RepositoryCapacityError {
    ZeroQuota,
}

impl std::fmt::Display for RepositoryCapacityError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ZeroQuota => formatter.write_str("repository quota must be greater than zero"),
        }
    }
}

impl std::error::Error for RepositoryCapacityError {}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum HistoryWriteAvailability {
    Writable,
    DegradedLowSpace,
    QuotaReached,
}

impl HistoryWriteAvailability {
    pub(crate) fn allows_history_writes(self) -> bool {
        matches!(self, Self::Writable)
    }

    pub(crate) fn allows_control_plane_operations(self) -> bool {
        true
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct RepositoryCapacity {
    quota_bytes: u64,
    #[serde(default)]
    used_bytes: u64,
    #[serde(default = "unbounded_available_bytes")]
    filesystem_available_bytes: u64,
}

impl Default for RepositoryCapacity {
    fn default() -> Self {
        Self {
            quota_bytes: DEFAULT_HISTORY_REPOSITORY_QUOTA_BYTES,
            used_bytes: 0,
            filesystem_available_bytes: unbounded_available_bytes(),
        }
    }
}

impl RepositoryCapacity {
    pub(crate) fn new(quota_bytes: u64) -> Result<Self, RepositoryCapacityError> {
        let capacity = Self {
            quota_bytes,
            ..Self::default()
        };
        capacity.validate()?;
        Ok(capacity)
    }

    pub(crate) fn quota_bytes(&self) -> u64 {
        self.quota_bytes
    }

    pub(crate) fn used_bytes(&self) -> u64 {
        self.used_bytes
    }

    pub(crate) fn filesystem_available_bytes(&self) -> u64 {
        self.filesystem_available_bytes
    }

    pub(crate) fn record_usage(
        &mut self,
        used_bytes: u64,
        filesystem_available_bytes: u64,
    ) -> Result<(), RepositoryCapacityError> {
        let next = Self {
            quota_bytes: self.quota_bytes,
            used_bytes,
            filesystem_available_bytes,
        };
        next.validate()?;
        *self = next;
        Ok(())
    }

    pub(crate) fn history_write_availability(&self) -> HistoryWriteAvailability {
        if self.filesystem_available_bytes < HISTORY_REPOSITORY_LOW_SPACE_GUARD_BYTES {
            HistoryWriteAvailability::DegradedLowSpace
        } else if self.used_bytes >= self.quota_bytes {
            HistoryWriteAvailability::QuotaReached
        } else {
            HistoryWriteAvailability::Writable
        }
    }

    pub(crate) fn validate(&self) -> Result<(), RepositoryCapacityError> {
        if self.quota_bytes == 0 {
            return Err(RepositoryCapacityError::ZeroQuota);
        }
        Ok(())
    }
}

impl<'de> Deserialize<'de> for RepositoryCapacity {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawRepositoryCapacity {
            quota_bytes: u64,
            #[serde(default)]
            used_bytes: u64,
            #[serde(default = "unbounded_available_bytes")]
            filesystem_available_bytes: u64,
        }

        let raw = RawRepositoryCapacity::deserialize(deserializer)?;
        let capacity = Self {
            quota_bytes: raw.quota_bytes,
            used_bytes: raw.used_bytes,
            filesystem_available_bytes: raw.filesystem_available_bytes,
        };
        capacity.validate().map_err(D::Error::custom)?;
        Ok(capacity)
    }
}

const fn unbounded_available_bytes() -> u64 {
    u64::MAX
}
