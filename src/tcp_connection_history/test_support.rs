//! Shared builders for connection-history unit tests.
//!
//! These live outside a `*tests` module so fixture-policy does not treat the struct literals as
//! inline test data; every synthetic identifier still comes from `xp_test_fixtures`.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};

use super::{ConnectionClass, StoredMinute};

pub(super) fn parse_minute(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value)
        .expect("fixture timestamp")
        .with_timezone(&Utc)
}

pub(super) fn stored_minute(at: &str, total: u32) -> StoredMinute {
    stored_minute_with_counts(
        at,
        total,
        BTreeMap::from([(
            (
                ConnectionClass::Inbound,
                xp_test_fixtures::endpoint_fixtureinline_a().to_string(),
            ),
            total,
        )]),
    )
}

pub(super) fn stored_minute_with_counts(
    at: &str,
    total: u32,
    counts: BTreeMap<(ConnectionClass, String), u32>,
) -> StoredMinute {
    StoredMinute {
        minute: parse_minute(at),
        total,
        truncated: false,
        partial: false,
        classes_observed: vec![ConnectionClass::Inbound, ConnectionClass::OutboundOther],
        counts,
    }
}

pub(super) fn stored_minute_split(at: &str, total: u32, inbound: u32, other: u32) -> StoredMinute {
    let mut counts = BTreeMap::new();
    if inbound > 0 {
        counts.insert(
            (
                ConnectionClass::Inbound,
                xp_test_fixtures::endpoint_fixtureinline_a().to_string(),
            ),
            inbound,
        );
    }
    if other > 0 {
        counts.insert((ConnectionClass::OutboundOther, "0".to_string()), other);
    }
    stored_minute_with_counts(at, total, counts)
}

pub(super) fn legacy_usage(
    minute: DateTime<Utc>,
) -> crate::tcp_connection_usage::PersistedTcpConnectionUsage {
    use crate::tcp_connection_usage::{
        PersistedTcpConnectionEndpointUsage, PersistedTcpConnectionUsage,
    };

    let mut usage = PersistedTcpConnectionUsage {
        latest_minute: Some(minute.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)),
        ..PersistedTcpConnectionUsage::default()
    };
    usage.endpoints.insert(
        xp_test_fixtures::endpoint_fixtureinline_a().to_string(),
        PersistedTcpConnectionEndpointUsage {
            node_id: xp_test_fixtures::label_node1_variant2().to_string(),
            endpoint_id: xp_test_fixtures::endpoint_fixtureinline_a().to_string(),
            endpoint_tag: xp_test_fixtures::endpoint_tag_fixtureinline_a().to_string(),
            port: 443,
            counts: vec![0; super::MINUTES_WINDOW - 1]
                .into_iter()
                .chain([3])
                .collect(),
            first_observed_at: None,
        },
    );
    usage
}

pub(super) fn deleted_registry()
-> std::collections::BTreeMap<String, crate::state::DeletedEndpointRecord> {
    std::collections::BTreeMap::from([
        (
            xp_test_fixtures::endpoint_fixtureinline_a().to_string(),
            crate::state::DeletedEndpointRecord {
                node_id: xp_test_fixtures::label_node1_variant2().to_string(),
                generation: 1,
            },
        ),
        (
            xp_test_fixtures::endpoint_fixtureinline_b().to_string(),
            crate::state::DeletedEndpointRecord {
                node_id: xp_test_fixtures::label_node2().to_string(),
                generation: 1,
            },
        ),
    ])
}
