use serde::{Deserialize, Serialize};

const DAY_SECONDS: u64 = 24 * 60 * 60;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RetentionResolution {
    Minute,
    FiveMinutes,
    Hour,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub(crate) struct RepositoryRetentionPolicy {
    minute_retention_seconds: u64,
    five_minute_retention_seconds: u64,
    max_retention_seconds: u64,
}

impl Default for RepositoryRetentionPolicy {
    fn default() -> Self {
        Self {
            minute_retention_seconds: 7 * DAY_SECONDS,
            five_minute_retention_seconds: 90 * DAY_SECONDS,
            max_retention_seconds: 2 * 365 * DAY_SECONDS,
        }
    }
}

impl RepositoryRetentionPolicy {
    pub(crate) fn minute_retention_seconds(self) -> u64 {
        self.minute_retention_seconds
    }

    pub(crate) fn five_minute_retention_seconds(self) -> u64 {
        self.five_minute_retention_seconds
    }

    pub(crate) fn max_age_seconds(self) -> u64 {
        self.max_retention_seconds
    }

    pub(crate) fn resolution_for_age(self, age_seconds: u64) -> Option<RetentionResolution> {
        if age_seconds <= self.minute_retention_seconds {
            Some(RetentionResolution::Minute)
        } else if age_seconds
            <= self
                .minute_retention_seconds
                .saturating_add(self.five_minute_retention_seconds)
        {
            Some(RetentionResolution::FiveMinutes)
        } else if age_seconds <= self.max_retention_seconds {
            Some(RetentionResolution::Hour)
        } else {
            None
        }
    }

    pub(crate) fn keeps_minute_detail(self, age_seconds: u64) -> bool {
        age_seconds <= self.minute_retention_seconds
    }

    pub(crate) const fn repository_only(self) -> bool {
        true
    }
}
