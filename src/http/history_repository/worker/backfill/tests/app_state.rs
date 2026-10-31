use crate::http::tests::{leader_raft, test_config, test_geo_db_update_handle, test_store_init};
use crate::http::*;

// Reuse the HTTP fixtures, but drive worker ticks explicitly so background replication cannot
// consume the pending wire or race the interruption under test.
pub(super) fn app_state(directory: &std::path::Path) -> AppState {
    let config = test_config(directory.to_path_buf());
    let cluster = ClusterMetadata::load(directory).unwrap_or_else(|_| {
        let cluster = ClusterMetadata::init_new_cluster(
            directory,
            config.node_name.clone(),
            config.access_host.clone(),
            config.api_base_url.clone(),
        )
        .unwrap();
        cluster.save(directory).unwrap();
        cluster
    });
    let cluster_ca_pem = cluster.read_cluster_ca_pem(directory).unwrap();
    let cluster_ca_key_pem = cluster.read_cluster_ca_key_pem(directory).unwrap();
    let mesh_client = build_mesh_http_client(
        &cluster_ca_pem,
        &cluster.read_node_cert_pem(directory).unwrap(),
        &cluster.read_node_key_pem(directory).unwrap(),
    )
    .unwrap();
    let store = Arc::new(Mutex::new(
        JsonSnapshotStore::load_or_init(test_store_init(&config, Some(cluster.node_id.clone())))
            .unwrap(),
    ));
    let raft = leader_raft(store.clone(), &cluster);
    let endpoint_probe = crate::endpoint_probe::new_endpoint_probe_handle(
        cluster.node_id.clone(),
        store.clone(),
        raft.clone(),
        "test-probe-secret".to_owned(),
        false,
    );
    AppState {
        node_runtime: NodeRuntimeHandle::from_config(&config, cluster.node_id.clone()),
        node_history: NodeHistoryHandle::from_config(&config),
        node_egress_probe: NodeEgressProbeHandle::new_noop(cluster.node_id.clone(), store.clone()),
        resource_monitoring: ResourceMonitorHandle::start(directory, cluster.node_id.clone()),
        repository_replica: Arc::new(Mutex::new(
            RepositoryReplicaRuntime::load(HistoryStorage::open(directory)).unwrap(),
        )),
        repository_query_gate: Arc::new(Semaphore::new(1)),
        repository_summary_gate: Arc::new(Semaphore::new(1)),
        geo_db_update: test_geo_db_update_handle(&config, store.clone()),
        uptime: crate::uptime_runtime::UptimeHandle::load(directory).unwrap(),
        mihomo_resource_policy: Arc::new(MihomoResourcePolicy::load(directory).unwrap()),
        mihomo_resource_directory: Arc::new(mihomo_resources::ResourceDirectoryCache::new()),
        mesh_telemetry: MeshTelemetryHandle::load(directory).unwrap(),
        internal_idempotency: InternalIdempotencyLedger::load(directory).unwrap(),
        config: Arc::new(config),
        store,
        reconcile: ReconcileHandle::noop(),
        xray_health: XrayHealthHandle::new_unknown(),
        cloudflared_health: CloudflaredHealthHandle::new_with_status(
            crate::cloudflared_supervisor::CloudflaredStatus::Disabled,
        ),
        endpoint_probe,
        cluster: Arc::new(cluster),
        cluster_ca_pem: Arc::new(cluster_ca_pem),
        cluster_ca_key_pem: Arc::new(cluster_ca_key_pem),
        raft,
        raft_rpc: None,
        version_check_cache: Arc::new(Mutex::new(VersionCheckCache::default())),
        ops_github_repo: Arc::new("IvanLi-CN/xp".to_owned()),
        ops_github_api_base_url: Arc::new("https://api.github.com".to_owned()),
        ops_github_client: reqwest::Client::new(),
        mesh_client,
        status_events: StatusEventsHub::new(),
        reverse_relay: crate::reverse_relay::ReverseRelayRuntime::default(),
        admin_token_verifier: AdminTokenVerifier::default(),
    }
}
