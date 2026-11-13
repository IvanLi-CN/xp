use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Default)]
pub struct TrafficCounter {
    uplink_bytes: AtomicU64,
    downlink_bytes: AtomicU64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrafficSnapshot {
    pub uplink_bytes: u64,
    pub downlink_bytes: u64,
}

impl TrafficCounter {
    pub fn add_uplink(&self, bytes: u64) {
        self.uplink_bytes.fetch_add(bytes, Ordering::Relaxed);
    }

    pub fn add_downlink(&self, bytes: u64) {
        self.downlink_bytes.fetch_add(bytes, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> TrafficSnapshot {
        TrafficSnapshot {
            uplink_bytes: self.uplink_bytes.load(Ordering::Relaxed),
            downlink_bytes: self.downlink_bytes.load(Ordering::Relaxed),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_uplink_and_downlink_bytes() {
        let counter = TrafficCounter::default();
        counter.add_uplink(7);
        counter.add_downlink(11);
        assert_eq!(
            counter.snapshot(),
            TrafficSnapshot {
                uplink_bytes: 7,
                downlink_bytes: 11,
            }
        );
    }
}
