use std::{collections::BTreeMap, collections::BTreeSet, sync::Arc};

use tokio::sync::{Mutex, watch};

use crate::domain::{Node, NodeQuotaReset};
use crate::raft::app::{LocalRaft, RaftFacade};
use crate::raft::types::{NodeMeta, raft_node_id_from_ulid};
use crate::state::{JsonSnapshotStore, StoreInit};

use crate::raft_membership_guard::node_rejoin::{absent_peers, preview_node_rejoin};

/// A leader whose Raft membership holds only the surviving voter, while DesiredState still lists
/// every node a quorum recovery retired. This is the exact shape rejoin exists for.
async fn absent_node_fixture(
    retired: &[(&str, &str)],
) -> (
    Arc<dyn RaftFacade>,
    Arc<Mutex<JsonSnapshotStore>>,
    tempfile::TempDir,
) {
    let temp = tempfile::tempdir().unwrap();
    let survivor_node_id = xp_test_fixtures::identifier_ulid_d().to_owned();
    let survivor_raft_id = raft_node_id_from_ulid(&survivor_node_id).unwrap();
    let store = Arc::new(Mutex::new(
        JsonSnapshotStore::load_or_init(StoreInit {
            data_dir: temp.path().to_path_buf(),
            bootstrap_node_id: Some(survivor_node_id.clone()),
            bootstrap_node_name: xp_test_fixtures::primary_node_name().to_owned(),
            bootstrap_access_host: xp_test_fixtures::primary_host().to_owned(),
            bootstrap_api_base_url: xp_test_fixtures::primary_api_url().to_owned(),
        })
        .unwrap(),
    ));
    for (node_id, node_name) in retired {
        store
            .lock()
            .await
            .upsert_node(Node {
                node_id: (*node_id).to_owned(),
                node_name: (*node_name).to_owned(),
                access_host: format!("{node_name}.example.test"),
                api_base_url: format!("https://{node_name}.example.test"),
                quota_limit_bytes: 0,
                quota_reset: NodeQuotaReset::default(),
            })
            .unwrap();
        store
            .lock()
            .await
            .create_endpoint(
                (*node_id).to_owned(),
                crate::domain::EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                8_388,
                serde_json::json!({}),
            )
            .unwrap();
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
                    name: xp_test_fixtures::primary_node_name().to_owned(),
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
    (raft, store, temp)
}

/// A retired node that still runs must be reported with the endpoint it keeps serving on, so the
/// operator can confirm the data plane is untouched before rejoining.
#[tokio::test]
async fn rejoin_preview_reports_the_absent_node_and_preserves_its_endpoint() {
    let retired_id = "01M7624MWVVMH7VZ2FARCB0HTD";
    let (raft, store, _temp) = absent_node_fixture(&[(retired_id, "us3")]).await;

    let preview = preview_node_rejoin(raft, store.clone(), retired_id)
        .await
        .expect("absent desired state node is a rejoin candidate");

    assert_eq!(preview.node_id, retired_id);
    assert_eq!(preview.node_name, "us3");
    assert_eq!(preview.api_base_url, "https://us3.example.test");
    assert_eq!(preview.endpoint_ids.len(), 1);
    // The endpoint is still owned by the retired node: rejoin never moves or deletes it.
    assert!(
        store
            .lock()
            .await
            .list_endpoints()
            .iter()
            .any(|endpoint| endpoint.node_id == retired_id)
    );
}

/// The leader already holds its identity; rejoining it is a caller mistake, not a recovery step.
#[tokio::test]
async fn rejoin_preview_rejects_the_current_leader() {
    let (raft, store, _temp) = absent_node_fixture(&[]).await;

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
    let (raft, store, _temp) = absent_node_fixture(&[]).await;

    let error = preview_node_rejoin(raft, store, "01M7Z2R0FNS864CYTMMWHJV120")
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
async fn absent_peers_names_every_remaining_absent_member() {
    let target = "01M7624MWVVMH7VZ2FARCB0HTD";
    let sibling = "01M7Z2R0FNS864CYTMMWHJV120";
    let (raft, store, _temp) = absent_node_fixture(&[(target, "us3"), (sibling, "us2")]).await;

    let peers = absent_peers(
        raft,
        store,
        raft_node_id_from_ulid(target).expect("target raft id"),
    )
    .await
    .expect("audit succeeds");

    assert_eq!(
        peers,
        vec![raft_node_id_from_ulid(sibling).unwrap().to_string()]
    );
}
