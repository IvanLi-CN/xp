use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use tokio::sync::{Mutex, watch};

use crate::domain::{Node, NodeQuotaReset};
use crate::raft::app::{LocalRaft, RaftFacade};
use crate::raft::types::{NodeMeta, raft_node_id_from_ulid};
use crate::raft_membership_guard::node_rejoin::{absent_peers, preview_node_rejoin};
use crate::state::{JsonSnapshotStore, StoreInit};

fn survivor_node() -> Node {
    Node {
        node_id: xp_test_fixtures::identifier_ulid_d().to_owned(),
        node_name: xp_test_fixtures::label_node1().to_owned(),
        access_host: xp_test_fixtures::primary_host().to_owned(),
        api_base_url: xp_test_fixtures::primary_api_url().to_owned(),
        quota_limit_bytes: 0,
        quota_reset: NodeQuotaReset::default(),
    }
}

fn absent_node() -> Node {
    Node {
        node_id: xp_test_fixtures::identifier_ulid_e().to_owned(),
        node_name: xp_test_fixtures::label_extra_node().to_owned(),
        access_host: xp_test_fixtures::secondary_host().to_owned(),
        api_base_url: xp_test_fixtures::secondary_api_url().to_owned(),
        quota_limit_bytes: 0,
        quota_reset: NodeQuotaReset::default(),
    }
}

/// A leader whose Raft membership holds only the surviving voter, while DesiredState still lists
/// every node a quorum recovery retired. This is the exact shape rejoin exists for.
async fn absent_node_fixture(
    include_absent_node: bool,
) -> (
    Arc<dyn RaftFacade>,
    Arc<Mutex<JsonSnapshotStore>>,
    tempfile::TempDir,
) {
    let temp = tempfile::tempdir().unwrap();
    let survivor_raft_id = raft_node_id_from_ulid(xp_test_fixtures::identifier_ulid_d()).unwrap();
    let store = Arc::new(Mutex::new(
        JsonSnapshotStore::load_or_init(StoreInit {
            data_dir: temp.path().to_path_buf(),
            bootstrap_node_id: Some(xp_test_fixtures::identifier_ulid_d().to_owned()),
            bootstrap_node_name: xp_test_fixtures::label_node1().to_owned(),
            bootstrap_access_host: xp_test_fixtures::primary_host().to_owned(),
            bootstrap_api_base_url: xp_test_fixtures::primary_api_url().to_owned(),
        })
        .unwrap(),
    ));
    if include_absent_node {
        store
            .lock()
            .await
            .upsert_node(absent_node())
            .expect("absent node");
        store
            .lock()
            .await
            .create_endpoint(
                xp_test_fixtures::identifier_ulid_e().to_owned(),
                crate::domain::EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                8_388,
                serde_json::json!({}),
            )
            .expect("absent endpoint");
    }

    let mut metrics = openraft::RaftMetrics::new_initial(survivor_raft_id);
    metrics.state = openraft::ServerState::Leader;
    metrics.current_leader = Some(survivor_raft_id);
    metrics.membership_config = Arc::new(openraft::StoredMembership::new(
        None,
        openraft::Membership::new(
            vec![BTreeSet::from([survivor_raft_id])],
            BTreeMap::from([(
                survivor_raft_id,
                NodeMeta {
                    name: xp_test_fixtures::label_node1().to_owned(),
                    api_base_url: xp_test_fixtures::primary_api_url().to_owned(),
                    raft_endpoint: xp_test_fixtures::primary_api_url().to_owned(),
                },
            )]),
        ),
    ));
    let (metrics_tx, metrics_rx) = watch::channel(metrics);
    // Keep the sender alive for the lifetime of the test's facade.
    std::mem::forget(metrics_tx);
    let raft: Arc<dyn RaftFacade> = Arc::new(LocalRaft::new(store.clone(), metrics_rx));
    let _ = survivor_node();
    (raft, store, temp)
}

/// A retired node that still runs must be reported with the endpoint it keeps serving on, so the
/// operator can confirm the data plane is untouched before rejoining.
#[tokio::test]
async fn rejoin_preview_reports_the_absent_node_and_preserves_its_endpoint() {
    let (raft, store, _temp) = absent_node_fixture(true).await;

    let preview = preview_node_rejoin(raft, store.clone(), xp_test_fixtures::identifier_ulid_e())
        .await
        .expect("absent desired state node is a rejoin candidate");

    assert_eq!(preview.node_id, xp_test_fixtures::identifier_ulid_e());
    assert_eq!(preview.node_name, xp_test_fixtures::label_extra_node());
    assert_eq!(preview.api_base_url, xp_test_fixtures::secondary_api_url());
    assert_eq!(preview.endpoint_ids.len(), 1);
    // The endpoint is still owned by the retired node: rejoin never moves or deletes it.
    assert!(
        store
            .lock()
            .await
            .list_endpoints()
            .iter()
            .any(|endpoint| endpoint.node_id == xp_test_fixtures::identifier_ulid_e())
    );
}

/// The leader already holds its identity; rejoining it is a caller mistake, not a recovery step.
#[tokio::test]
async fn rejoin_preview_rejects_the_current_leader() {
    let (raft, store, _temp) = absent_node_fixture(false).await;

    let error = preview_node_rejoin(raft, store, xp_test_fixtures::identifier_ulid_d())
        .await
        .expect_err("the leader already holds its Raft identity");

    assert!(
        error.to_string().contains("is the current leader"),
        "unexpected error: {error}"
    );
}

/// A node that is not in DesiredState cannot be rejoined: rejoin restores an existing identity, it
/// never creates one.
#[tokio::test]
async fn rejoin_preview_rejects_a_node_outside_desired_state() {
    let (raft, store, _temp) = absent_node_fixture(false).await;

    let error = preview_node_rejoin(raft, store, xp_test_fixtures::identifier_ulid_e())
        .await
        .expect_err("unknown node is not a rejoin candidate");

    assert!(
        error
            .to_string()
            .contains("not represented by desired state"),
        "unexpected error: {error}"
    );
}

/// Every other absent member must be named, because the restore admission compares the full
/// `missing_desired_members` set and rejoin must not silently excuse anything.
#[tokio::test]
async fn absent_peers_names_the_remaining_absent_member() {
    let (raft, store, _temp) = absent_node_fixture(true).await;

    let peers = absent_peers(
        raft,
        store,
        raft_node_id_from_ulid(xp_test_fixtures::identifier_ulid_d()).expect("survivor raft id"),
    )
    .await
    .expect("audit succeeds");

    // Reported as the full ULID: a raft id is only the low 64 bits and cannot be reversed, so the
    // caller needs the stable node id it can pass to `rejoin-node` or an eviction.
    assert_eq!(
        peers,
        vec![xp_test_fixtures::identifier_ulid_e().to_owned()]
    );
}
