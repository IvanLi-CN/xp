use serde::{Deserialize, Serialize};

pub const ACTIVATION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10 * 60);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JoinSessionStatus {
    Reserved,
    LearnerRegistered,
    Consumed,
    Expired,
}

impl JoinSessionStatus {
    pub fn is_pending(&self) -> bool {
        matches!(self, Self::Reserved | Self::LearnerRegistered)
    }

    fn may_transition_to(&self, next: &Self) -> bool {
        matches!(
            (self, next),
            (
                Self::Reserved,
                Self::Reserved | Self::LearnerRegistered | Self::Expired
            ) | (
                Self::LearnerRegistered,
                Self::LearnerRegistered | Self::Consumed | Self::Expired
            ) | (Self::Consumed, Self::Consumed)
                | (Self::Expired, Self::Expired)
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JoinSession {
    pub node_id: String,
    pub request_fingerprint: String,
    pub signed_cert_pem: String,
    pub token_expires_at: String,
    pub activation_deadline: String,
    pub required_log_index: u64,
    pub status: JoinSessionStatus,
    pub terminal_at: Option<String>,
}

impl JoinSession {
    pub fn request_fingerprint(
        node_name: &str,
        access_host: &str,
        api_base_url: &str,
        csr_pem: &str,
    ) -> Result<String, serde_json::Error> {
        use sha2::Digest as _;
        let bytes = serde_json::to_vec(&serde_json::json!({
            "node_name": node_name,
            "access_host": access_host,
            "api_base_url": api_base_url,
            "csr_pem": csr_pem,
        }))?;
        Ok(hex::encode(sha2::Sha256::digest(bytes)))
    }

    pub fn learner_registered(mut self, required_log_index: u64) -> Self {
        self.required_log_index = self.required_log_index.max(required_log_index);
        self.status = JoinSessionStatus::LearnerRegistered;
        self
    }

    pub fn validate_successor(&self, next: &Self) -> Result<(), &'static str> {
        if self.node_id != next.node_id
            || self.request_fingerprint != next.request_fingerprint
            || self.signed_cert_pem != next.signed_cert_pem
            || self.token_expires_at != next.token_expires_at
            || self.activation_deadline != next.activation_deadline
        {
            return Err("immutable join session identity changed");
        }
        if !self.status.may_transition_to(&next.status) {
            return Err("invalid join session status transition");
        }
        if next.required_log_index < self.required_log_index {
            return Err("join session required log index moved backwards");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use super::*;

    fn session(status: JoinSessionStatus) -> JoinSession {
        JoinSession {
            node_id: xp_test_fixtures::primary_node_id().into(),
            request_fingerprint: "fingerprint".into(),
            signed_cert_pem: "certificate".into(),
            token_expires_at: "2026-08-16T00:00:00Z".into(),
            activation_deadline: "2026-08-16T00:10:00Z".into(),
            required_log_index: 0,
            status,
            terminal_at: None,
        }
    }

    #[test]
    fn allows_only_forward_idempotent_transitions() {
        let reserved = session(JoinSessionStatus::Reserved);
        let mut learner = session(JoinSessionStatus::LearnerRegistered);
        learner.required_log_index = 42;
        assert!(reserved.validate_successor(&learner).is_ok());

        let mut consumed = learner.clone();
        consumed.status = JoinSessionStatus::Consumed;
        consumed.terminal_at = Some("2026-08-16T00:01:00Z".into());
        assert!(learner.validate_successor(&consumed).is_ok());
        assert!(consumed.validate_successor(&consumed).is_ok());
        assert!(consumed.validate_successor(&learner).is_err());
    }

    #[test]
    fn rejects_identity_changes_and_log_regressions() {
        let current = session(JoinSessionStatus::LearnerRegistered);
        let mut conflicting = current.clone();
        conflicting.request_fingerprint = "other".into();
        assert!(current.validate_successor(&conflicting).is_err());

        let mut regressed = current.clone();
        regressed.required_log_index = 1;
        let mut next = regressed.clone();
        next.required_log_index = 0;
        assert!(regressed.validate_successor(&next).is_err());
    }

    #[test]
    fn session_is_additive_to_legacy_upsert_node_command() {
        #[derive(Deserialize)]
        #[serde(tag = "type", rename_all = "snake_case")]
        enum LegacyCommand {
            UpsertNode { node: crate::domain::Node },
        }
        let node = crate::domain::Node {
            node_id: xp_test_fixtures::primary_node_id().to_owned(),
            node_name: xp_test_fixtures::primary_node_name().to_owned(),
            access_host: xp_test_fixtures::host_fixture465().to_owned(),
            api_base_url: xp_test_fixtures::primary_api_url().to_owned(),
            quota_limit_bytes: 0,
            quota_reset: Default::default(),
        };
        let command = crate::state::DesiredStateCommand::UpsertNode {
            node: node.clone(),
            join_session: Some(session(JoinSessionStatus::Reserved)),
        };
        let legacy: LegacyCommand =
            serde_json::from_value(serde_json::to_value(command).unwrap()).unwrap();
        let LegacyCommand::UpsertNode { node: decoded } = legacy;
        assert_eq!(decoded, node);
    }
}
