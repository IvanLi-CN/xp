use std::time::{Duration, Instant};

use crate::transport::XmuxSettings;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamPermit {
    pub connection_id: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReuseDecision {
    pub reusable: bool,
    pub reason: ReuseReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReuseReason {
    Allowed,
    MaxConnectionReuseTimes,
    MaxRequestTimes,
    ReusableLifetimeExpired,
}

#[derive(Debug)]
pub struct StreamOneSession {
    path: String,
    settings: XmuxSettings,
    next_connection_id: u64,
    active_connections: u16,
    active_streams: u16,
    connection_reuses: u32,
    request_count: u32,
    created_at: Instant,
}

impl StreamOneSession {
    pub fn new(path: impl Into<String>, settings: XmuxSettings) -> Result<Self, String> {
        let path = path.into();
        if !path.starts_with('/') || path.contains("..") {
            return Err("XHTTP path must be absolute and cannot contain '..'".into());
        }
        if settings.max_connections == 0 {
            return Err("XMUX max_connections must be non-zero".into());
        }
        Ok(Self {
            path,
            settings,
            next_connection_id: 1,
            active_connections: 0,
            active_streams: 0,
            connection_reuses: 0,
            request_count: 0,
            created_at: Instant::now(),
        })
    }

    pub fn accepts_path(&self, request_path: &str) -> bool {
        request_path == self.path
            || request_path
                .strip_prefix(&self.path)
                .is_some_and(|suffix| suffix.starts_with('/'))
    }

    pub fn try_open_stream(&mut self) -> Result<StreamPermit, String> {
        if self.settings.max_concurrency != 0
            && self.active_streams >= self.settings.max_concurrency
        {
            return Err("XMUX max_concurrency reached".into());
        }
        if self.active_connections >= self.settings.max_connections {
            return Err("XMUX max_connections reached".into());
        }
        let connection_id = self.next_connection_id;
        self.next_connection_id = self.next_connection_id.saturating_add(1);
        self.active_connections += 1;
        self.active_streams += 1;
        self.request_count = self.request_count.saturating_add(1);
        Ok(StreamPermit { connection_id })
    }

    pub fn close_stream(&mut self, _permit: StreamPermit) -> ReuseDecision {
        self.active_connections = self.active_connections.saturating_sub(1);
        self.active_streams = self.active_streams.saturating_sub(1);
        self.connection_reuses = self.connection_reuses.saturating_add(1);
        match self.reuse_decision() {
            Ok(decision) | Err(decision) => decision,
        }
    }

    pub fn reuse_decision(&self) -> Result<ReuseDecision, ReuseDecision> {
        if self.settings.c_max_reuse_times != 0
            && self.connection_reuses >= self.settings.c_max_reuse_times
        {
            return Err(ReuseDecision {
                reusable: false,
                reason: ReuseReason::MaxConnectionReuseTimes,
            });
        }
        if self.settings.h_max_request_times != 0
            && self.request_count >= self.settings.h_max_request_times
        {
            return Err(ReuseDecision {
                reusable: false,
                reason: ReuseReason::MaxRequestTimes,
            });
        }
        if self.settings.h_max_reusable_secs != 0
            && self.created_at.elapsed()
                >= Duration::from_secs(self.settings.h_max_reusable_secs as u64)
        {
            return Err(ReuseDecision {
                reusable: false,
                reason: ReuseReason::ReusableLifetimeExpired,
            });
        }
        Ok(ReuseDecision {
            reusable: true,
            reason: ReuseReason::Allowed,
        })
    }

    pub fn active_streams(&self) -> u16 {
        self.active_streams
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> XmuxSettings {
        XmuxSettings {
            max_connections: 2,
            max_concurrency: 1,
            c_max_reuse_times: 2,
            h_max_request_times: 3,
            h_max_reusable_secs: 60,
            h_keep_alive_period: -1,
        }
    }

    #[test]
    fn stream_one_enforces_concurrency_and_path_boundary() {
        let mut session = StreamOneSession::new("/xp-xhttp", settings()).unwrap();
        assert!(session.accepts_path("/xp-xhttp"));
        assert!(session.accepts_path("/xp-xhttp/child"));
        assert!(!session.accepts_path("/xp-xhttp-other"));
        let permit = session.try_open_stream().unwrap();
        assert!(session.try_open_stream().is_err());
        assert_eq!(session.active_streams(), 1);
        assert_eq!(session.close_stream(permit).reason, ReuseReason::Allowed);
    }

    #[test]
    fn request_limit_closes_reuse_after_configured_count() {
        let mut limited = settings();
        limited.c_max_reuse_times = 0;
        let mut session = StreamOneSession::new("/xp-xhttp", limited).unwrap();
        for _ in 0..3 {
            let permit = session.try_open_stream().unwrap();
            session.close_stream(permit);
        }
        assert_eq!(
            session.reuse_decision().unwrap_err().reason,
            ReuseReason::MaxRequestTimes
        );
    }
}
