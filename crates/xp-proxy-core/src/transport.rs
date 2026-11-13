use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RealityConfig {
    pub server_names: Vec<String>,
    pub short_ids: Vec<String>,
    pub private_key: [u8; 32],
    pub dest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct XmuxSettings {
    pub max_connections: u16,
    pub max_concurrency: u16,
    pub c_max_reuse_times: u32,
    pub h_max_request_times: u32,
    pub h_max_reusable_secs: u32,
    pub h_keep_alive_period: i32,
}

impl Default for XmuxSettings {
    fn default() -> Self {
        Self {
            max_connections: 1,
            max_concurrency: 0,
            c_max_reuse_times: 0,
            h_max_request_times: 0,
            h_max_reusable_secs: 0,
            h_keep_alive_period: -1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct XhttpConfig {
    pub path: String,
    pub mode: String,
    pub xmux: XmuxSettings,
}

impl XhttpConfig {
    pub fn validate(&self) -> Result<(), String> {
        if !self.path.starts_with('/')
            || self.path.contains("..")
            || self.path.contains('?')
            || self.path.contains('#')
            || self.path.chars().any(|ch| ch.is_ascii_whitespace())
        {
            return Err("XHTTP path must be absolute and cannot contain '..'".into());
        }
        if self.mode != "stream-one" {
            return Err(format!("unsupported XHTTP mode: {}", self.mode));
        }
        if self.xmux.max_connections == 0 {
            return Err("XMUX max_connections must be non-zero".into());
        }
        if self.xmux.h_keep_alive_period < -1 {
            return Err("XMUX h_keep_alive_period must be -1 or non-negative".into());
        }
        Ok(())
    }
}

impl RealityConfig {
    pub fn validate(&self) -> Result<(), String> {
        if self.server_names.is_empty() || self.short_ids.is_empty() {
            return Err("REALITY server_names and short_ids must not be empty".into());
        }
        if self
            .short_ids
            .iter()
            .any(|id| id.len() != 16 || !id.bytes().all(|b| b.is_ascii_hexdigit()))
        {
            return Err("REALITY short_id must be 16 hexadecimal characters".into());
        }
        if self.dest.parse::<std::net::SocketAddr>().is_err() && self.dest.is_empty() {
            return Err("REALITY dest must not be empty".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_stream_one_is_accepted() {
        let config = XhttpConfig {
            path: "/xp-xhttp".into(),
            mode: "stream-up".into(),
            xmux: XmuxSettings::default(),
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn invalid_short_id_is_rejected() {
        let config = RealityConfig {
            server_names: vec!["example.test".into()],
            short_ids: vec!["bad".into()],
            private_key: [0; 32],
            dest: "127.0.0.1:443".into(),
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn xhttp_rejects_query_fragment_and_invalid_keep_alive() {
        let mut config = XhttpConfig {
            path: "/xp-xhttp?bad".into(),
            mode: "stream-one".into(),
            xmux: XmuxSettings::default(),
        };
        assert!(config.validate().is_err());
        config.path = "/xp-xhttp".into();
        config.xmux.h_keep_alive_period = -2;
        assert!(config.validate().is_err());
    }
}
