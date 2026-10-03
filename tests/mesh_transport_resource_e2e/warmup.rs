macro_rules! wait_for_peer_fleet_warmup {
    ($fleet:expr, $label:expr) => {{
        if $label == "candidate-smoke" {
            let ready = ::tokio::time::timeout(::std::time::Duration::from_secs(30), async {
                while !$fleet.counters.iter().all(|counter| {
                    counter.requests.load(::std::sync::atomic::Ordering::SeqCst) >= 1
                }) {
                    ::tokio::time::sleep(::std::time::Duration::from_millis(100)).await;
                }
            })
            .await;
            assert!(
                ready.is_ok(),
                "Mesh peer fleet did not warm up for {}",
                $label
            );
        }
    }};
}

pub(super) use wait_for_peer_fleet_warmup;
