use std::{
    collections::BTreeMap,
    fmt::Debug,
    ops::RangeBounds,
    path::{Path, PathBuf},
    sync::Arc,
};

use tokio::sync::Mutex;

use crate::{
    raft::types::ClientResponse,
    raft::types::{NodeId, NodeMeta, TypeConfig},
    reconcile::ReconcileHandle,
    state::{DesiredStateCommand, JsonSnapshotStore},
};

use openraft::entry::RaftPayload as _;
use openraft::{
    EntryPayload, ErrorSubject, ErrorVerb, LogId, LogState, RaftLogReader, Snapshot, SnapshotMeta,
    StoredMembership, Vote,
    storage::{RaftLogStorage, RaftStateMachine},
};

#[derive(Debug, Clone)]
pub struct StorePaths {
    pub wal_json: PathBuf,
    pub vote_json: PathBuf,
    pub committed_json: PathBuf,
    pub sm_meta_json: PathBuf,
    pub snapshot_meta_json: PathBuf,
    pub snapshot_data_json: PathBuf,
}

impl StorePaths {
    pub fn new(data_dir: &Path) -> Self {
        let raft_dir = data_dir.join("raft");
        let wal_dir = raft_dir.join("wal");
        let snapshot_dir = raft_dir.join("snapshots");
        Self {
            wal_json: wal_dir.join("log.json"),
            vote_json: wal_dir.join("vote.json"),
            committed_json: wal_dir.join("committed.json"),
            sm_meta_json: raft_dir.join("state_machine.json"),
            snapshot_meta_json: snapshot_dir.join("current_meta.json"),
            snapshot_data_json: snapshot_dir.join("current_snapshot.json"),
        }
    }

    pub fn ensure_dirs(&self) -> std::io::Result<()> {
        if let Some(parent) = self.wal_json.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if let Some(parent) = self.snapshot_meta_json.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Ok(())
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct PersistedWal {
    #[serde(default)]
    last_purged_log_id: Option<LogId<NodeId>>,
    #[serde(default)]
    entries: Vec<openraft::impls::Entry<TypeConfig>>,
}

impl PersistedWal {
    fn empty() -> Self {
        Self {
            last_purged_log_id: None,
            entries: Vec::new(),
        }
    }
}

#[derive(Debug)]
struct WalInner {
    last_purged_log_id: Option<LogId<NodeId>>,
    entries: BTreeMap<u64, openraft::impls::Entry<TypeConfig>>,
    vote: Option<Vote<NodeId>>,
    committed: Option<LogId<NodeId>>,
}

impl WalInner {
    fn last_log_id(&self) -> Option<LogId<NodeId>> {
        self.entries
            .iter()
            .next_back()
            .map(|(_idx, ent)| ent.log_id)
            .or(self.last_purged_log_id)
    }
}

#[derive(Debug, Clone)]
pub struct FileLogStore {
    paths: StorePaths,
    inner: Arc<Mutex<WalInner>>,
}

impl FileLogStore {
    pub async fn open(
        data_dir: &Path,
        _node_id: NodeId,
    ) -> Result<Self, openraft::StorageError<NodeId>> {
        let paths = StorePaths::new(data_dir);
        paths
            .ensure_dirs()
            .map_err(|e| io_err(ErrorSubject::Store, ErrorVerb::Write, e))?;

        let sm_meta = read_json::<PersistedStateMachineMeta>(&paths.sm_meta_json)
            .await
            .map_err(|e| io_err(ErrorSubject::StateMachine, ErrorVerb::Read, e))?;
        let last_applied_index = sm_meta
            .as_ref()
            .and_then(|meta| meta.last_applied.as_ref().map(|log_id| log_id.index));

        let (wal, wal_rewritten) = read_wal_with_compat(&paths.wal_json, last_applied_index)
            .await
            .map_err(|e| io_err(ErrorSubject::Logs, ErrorVerb::Read, e))?;
        let vote = read_json::<Vote<NodeId>>(&paths.vote_json)
            .await
            .map_err(|e| io_err(ErrorSubject::Vote, ErrorVerb::Read, e))?;
        let committed = read_json::<LogId<NodeId>>(&paths.committed_json)
            .await
            .map_err(|e| io_err(ErrorSubject::Store, ErrorVerb::Read, e))?;

        let entries = wal
            .entries
            .into_iter()
            .map(|ent| (ent.log_id.index, ent))
            .collect::<BTreeMap<_, _>>();

        if wal_rewritten {
            write_json(
                &paths.wal_json,
                &PersistedWal {
                    last_purged_log_id: wal.last_purged_log_id,
                    entries: entries.values().cloned().collect(),
                },
            )
            .await
            .map_err(|e| io_err(ErrorSubject::Logs, ErrorVerb::Write, e))?;
        }

        Ok(Self {
            paths,
            inner: Arc::new(Mutex::new(WalInner {
                last_purged_log_id: wal.last_purged_log_id,
                entries,
                vote,
                committed,
            })),
        })
    }

    async fn persist_wal(&self) -> Result<(), openraft::StorageError<NodeId>> {
        let inner = self.inner.lock().await;
        let wal = PersistedWal {
            last_purged_log_id: inner.last_purged_log_id,
            entries: inner.entries.values().cloned().collect(),
        };
        write_json(&self.paths.wal_json, &wal)
            .await
            .map_err(|e| io_err(ErrorSubject::Logs, ErrorVerb::Write, e))?;
        Ok(())
    }

    async fn persist_vote(&self) -> Result<(), openraft::StorageError<NodeId>> {
        let inner = self.inner.lock().await;
        if let Some(vote) = &inner.vote {
            write_json(&self.paths.vote_json, vote)
                .await
                .map_err(|e| io_err(ErrorSubject::Vote, ErrorVerb::Write, e))?;
        }
        Ok(())
    }

    async fn persist_committed(&self) -> Result<(), openraft::StorageError<NodeId>> {
        let inner = self.inner.lock().await;
        if let Some(committed) = &inner.committed {
            write_json(&self.paths.committed_json, committed)
                .await
                .map_err(|e| io_err(ErrorSubject::Store, ErrorVerb::Write, e))?;
        }
        Ok(())
    }
}

impl RaftLogReader<TypeConfig> for FileLogStore {
    async fn try_get_log_entries<RB: RangeBounds<u64> + Clone + Debug + openraft::OptionalSend>(
        &mut self,
        range: RB,
    ) -> Result<Vec<openraft::impls::Entry<TypeConfig>>, openraft::StorageError<NodeId>> {
        let inner = self.inner.lock().await;
        let mut out = Vec::new();
        for (_idx, ent) in inner.entries.range(range) {
            out.push(ent.clone());
        }
        Ok(out)
    }
}

impl RaftLogStorage<TypeConfig> for FileLogStore {
    type LogReader = FileLogStore;

    async fn get_log_state(
        &mut self,
    ) -> Result<LogState<TypeConfig>, openraft::StorageError<NodeId>> {
        let inner = self.inner.lock().await;
        Ok(LogState {
            last_purged_log_id: inner.last_purged_log_id,
            last_log_id: inner.last_log_id(),
        })
    }

    async fn get_log_reader(&mut self) -> Self::LogReader {
        self.clone()
    }

    async fn save_vote(
        &mut self,
        vote: &Vote<NodeId>,
    ) -> Result<(), openraft::StorageError<NodeId>> {
        {
            let mut inner = self.inner.lock().await;
            inner.vote = Some(*vote);
        }
        self.persist_vote().await?;
        Ok(())
    }

    async fn read_vote(&mut self) -> Result<Option<Vote<NodeId>>, openraft::StorageError<NodeId>> {
        let inner = self.inner.lock().await;
        Ok(inner.vote)
    }

    async fn save_committed(
        &mut self,
        committed: Option<LogId<NodeId>>,
    ) -> Result<(), openraft::StorageError<NodeId>> {
        {
            let mut inner = self.inner.lock().await;
            inner.committed = committed;
        }
        self.persist_committed().await?;
        Ok(())
    }

    async fn read_committed(
        &mut self,
    ) -> Result<Option<LogId<NodeId>>, openraft::StorageError<NodeId>> {
        let inner = self.inner.lock().await;
        Ok(inner.committed)
    }

    async fn append<I>(
        &mut self,
        entries: I,
        callback: openraft::storage::LogFlushed<TypeConfig>,
    ) -> Result<(), openraft::StorageError<NodeId>>
    where
        I: IntoIterator<Item = openraft::impls::Entry<TypeConfig>> + openraft::OptionalSend,
        I::IntoIter: openraft::OptionalSend,
    {
        {
            let mut inner = self.inner.lock().await;
            for ent in entries {
                inner.entries.insert(ent.log_id.index, ent);
            }
        }

        let res = self.persist_wal().await;
        callback.log_io_completed(
            res.as_ref()
                .map(|_| ())
                .map_err(|e| std::io::Error::other(e.to_string())),
        );
        res
    }

    async fn truncate(
        &mut self,
        log_id: LogId<NodeId>,
    ) -> Result<(), openraft::StorageError<NodeId>> {
        {
            let mut inner = self.inner.lock().await;
            inner.entries.split_off(&log_id.index);
        }
        self.persist_wal().await
    }

    async fn purge(&mut self, log_id: LogId<NodeId>) -> Result<(), openraft::StorageError<NodeId>> {
        {
            let mut inner = self.inner.lock().await;
            let keys: Vec<u64> = inner
                .entries
                .range(..=log_id.index)
                .map(|(k, _)| *k)
                .collect();
            for k in keys {
                inner.entries.remove(&k);
            }
            inner.last_purged_log_id = Some(log_id);
        }
        self.persist_wal().await
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct PersistedStateMachineMeta {
    last_applied: Option<LogId<NodeId>>,
    last_membership: StoredMembership<NodeId, NodeMeta>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct SnapshotPayload {
    state: crate::state::PersistedState,
}

#[derive(Debug)]
struct StateMachineInner {
    last_applied: Option<LogId<NodeId>>,
    last_membership: StoredMembership<NodeId, NodeMeta>,
}

#[derive(Debug, Clone)]
pub struct FileStateMachine {
    store: Arc<Mutex<JsonSnapshotStore>>,
    reconcile: ReconcileHandle,
    paths: StorePaths,
    inner: Arc<Mutex<StateMachineInner>>,
}

impl FileStateMachine {
    pub async fn open(
        data_dir: &Path,
        store: Arc<Mutex<JsonSnapshotStore>>,
        reconcile: ReconcileHandle,
    ) -> Result<Self, openraft::StorageError<NodeId>> {
        let paths = StorePaths::new(data_dir);
        paths
            .ensure_dirs()
            .map_err(|e| io_err(ErrorSubject::Store, ErrorVerb::Write, e))?;

        let meta = read_json::<PersistedStateMachineMeta>(&paths.sm_meta_json)
            .await
            .map_err(|e| io_err(ErrorSubject::StateMachine, ErrorVerb::Read, e))?;

        let (last_applied, last_membership) = meta
            .map(|m| (m.last_applied, m.last_membership))
            .unwrap_or((None, StoredMembership::default()));

        Ok(Self {
            store,
            reconcile,
            paths,
            inner: Arc::new(Mutex::new(StateMachineInner {
                last_applied,
                last_membership,
            })),
        })
    }

    async fn persist_meta(&self) -> Result<(), openraft::StorageError<NodeId>> {
        let inner = self.inner.lock().await;
        let meta = PersistedStateMachineMeta {
            last_applied: inner.last_applied,
            last_membership: inner.last_membership.clone(),
        };
        write_json(&self.paths.sm_meta_json, &meta)
            .await
            .map_err(|e| io_err(ErrorSubject::StateMachine, ErrorVerb::Write, e))?;
        Ok(())
    }
}

#[derive(Debug)]
pub struct FileSnapshotBuilder {
    store: Arc<Mutex<JsonSnapshotStore>>,
    inner: Arc<Mutex<StateMachineInner>>,
    paths: StorePaths,
}

impl openraft::RaftSnapshotBuilder<TypeConfig> for FileSnapshotBuilder {
    async fn build_snapshot(
        &mut self,
    ) -> Result<Snapshot<TypeConfig>, openraft::StorageError<NodeId>> {
        let (last_applied, last_membership) = {
            let inner = self.inner.lock().await;
            (inner.last_applied, inner.last_membership.clone())
        };

        let state = {
            let store = self.store.lock().await;
            store.state().clone()
        };

        let payload = SnapshotPayload { state };
        let bytes = serde_json::to_vec_pretty(&payload).map_err(|e| {
            io_err(
                ErrorSubject::Snapshot(None),
                ErrorVerb::Write,
                std::io::Error::other(e),
            )
        })?;

        let meta = SnapshotMeta {
            last_log_id: last_applied,
            last_membership,
            snapshot_id: format!(
                "snapshot-{}",
                last_applied.as_ref().map(|l| l.index).unwrap_or(0)
            ),
        };

        write_json(&self.paths.snapshot_meta_json, &meta)
            .await
            .map_err(|e| io_err(ErrorSubject::Snapshot(None), ErrorVerb::Write, e))?;
        write_bytes(&self.paths.snapshot_data_json, &bytes)
            .await
            .map_err(|e| io_err(ErrorSubject::Snapshot(None), ErrorVerb::Write, e))?;

        Ok(Snapshot {
            meta,
            snapshot: Box::new(std::io::Cursor::new(bytes)),
        })
    }
}

impl RaftStateMachine<TypeConfig> for FileStateMachine {
    type SnapshotBuilder = FileSnapshotBuilder;

    async fn applied_state(
        &mut self,
    ) -> Result<
        (Option<LogId<NodeId>>, StoredMembership<NodeId, NodeMeta>),
        openraft::StorageError<NodeId>,
    > {
        let inner = self.inner.lock().await;
        Ok((inner.last_applied, inner.last_membership.clone()))
    }

    async fn apply<I>(
        &mut self,
        entries: I,
    ) -> Result<Vec<ClientResponse>, openraft::StorageError<NodeId>>
    where
        I: IntoIterator<Item = openraft::impls::Entry<TypeConfig>> + openraft::OptionalSend,
        I::IntoIter: openraft::OptionalSend,
    {
        let mut responses = Vec::new();

        for entry in entries {
            let log_id = entry.log_id;
            if let Some(membership) = entry.get_membership() {
                let mut inner = self.inner.lock().await;
                inner.last_membership = StoredMembership::new(Some(log_id), membership.clone());
            }

            let resp = match entry.payload {
                EntryPayload::Normal(cmd) => {
                    let mut store = self.store.lock().await;
                    let rebuild_inbound = match &cmd {
                        DesiredStateCommand::UpsertEndpoint { endpoint } => store
                            .get_endpoint(&endpoint.endpoint_id)
                            .filter(|existing| {
                                existing.port != endpoint.port || existing.meta != endpoint.meta
                            })
                            .map(|_| endpoint.endpoint_id.clone()),
                        _ => None,
                    };
                    let membership_keys_before: Option<std::collections::BTreeSet<String>> =
                        match &cmd {
                            DesiredStateCommand::ReplaceUserAccess { user_id, .. }
                            | DesiredStateCommand::DeleteUser { user_id } => Some(
                                store
                                    .state()
                                    .node_user_endpoint_memberships
                                    .iter()
                                    .filter(|m| m.user_id == *user_id)
                                    .map(|m| {
                                        crate::state::membership_key(&m.user_id, &m.endpoint_id)
                                    })
                                    .collect(),
                            ),
                            DesiredStateCommand::DeleteEndpoint { endpoint_id } => Some(
                                store
                                    .state()
                                    .node_user_endpoint_memberships
                                    .iter()
                                    .filter(|m| m.endpoint_id == *endpoint_id)
                                    .map(|m| {
                                        crate::state::membership_key(&m.user_id, &m.endpoint_id)
                                    })
                                    .collect(),
                            ),
                            _ => None,
                        };
                    match cmd.apply(store.state_mut()) {
                        Ok(apply_result) => {
                            store.save().map_err(|e| {
                                io_err(
                                    ErrorSubject::StateMachine,
                                    ErrorVerb::Write,
                                    std::io::Error::other(e.to_string()),
                                )
                            })?;
                            if let Some(endpoint_id) = rebuild_inbound {
                                self.reconcile.request_rebuild_inbound(endpoint_id);
                            }

                            if let Some(before) = membership_keys_before {
                                let after: std::collections::BTreeSet<String> = match &cmd {
                                    DesiredStateCommand::ReplaceUserAccess { user_id, .. }
                                    | DesiredStateCommand::DeleteUser { user_id } => store
                                        .state()
                                        .node_user_endpoint_memberships
                                        .iter()
                                        .filter(|m| m.user_id == *user_id)
                                        .map(|m| {
                                            crate::state::membership_key(&m.user_id, &m.endpoint_id)
                                        })
                                        .collect(),
                                    DesiredStateCommand::DeleteEndpoint { endpoint_id } => store
                                        .state()
                                        .node_user_endpoint_memberships
                                        .iter()
                                        .filter(|m| m.endpoint_id == *endpoint_id)
                                        .map(|m| {
                                            crate::state::membership_key(&m.user_id, &m.endpoint_id)
                                        })
                                        .collect(),
                                    _ => std::collections::BTreeSet::new(),
                                };

                                for membership_key in before.difference(&after) {
                                    store.clear_membership_usage(membership_key).map_err(|e| {
                                        io_err(
                                            ErrorSubject::StateMachine,
                                            ErrorVerb::Write,
                                            std::io::Error::other(e.to_string()),
                                        )
                                    })?;
                                    store
                                        .clear_membership_inbound_ip_usage(membership_key)
                                        .map_err(|e| {
                                            io_err(
                                                ErrorSubject::StateMachine,
                                                ErrorVerb::Write,
                                                std::io::Error::other(e.to_string()),
                                            )
                                        })?;
                                }
                            }

                            ClientResponse::Ok {
                                result: apply_result,
                            }
                        }
                        Err(crate::state::StoreError::Domain(domain)) => {
                            let code = domain.code();
                            let status = match code {
                                "not_found" => 404,
                                "conflict" => 409,
                                _ => 400,
                            };
                            ClientResponse::Err {
                                status,
                                code: code.to_string(),
                                message: domain.to_string(),
                            }
                        }
                        Err(err) => {
                            return Err(io_err(
                                ErrorSubject::StateMachine,
                                ErrorVerb::Write,
                                std::io::Error::other(err.to_string()),
                            ));
                        }
                    }
                }
                EntryPayload::Membership(_) | EntryPayload::Blank => ClientResponse::Ok {
                    result: crate::state::DesiredStateApplyResult::Applied,
                },
            };

            {
                let mut inner = self.inner.lock().await;
                inner.last_applied = Some(log_id);
            }

            responses.push(resp);
        }

        self.persist_meta().await?;
        self.reconcile.request_full();
        Ok(responses)
    }

    async fn get_snapshot_builder(&mut self) -> Self::SnapshotBuilder {
        FileSnapshotBuilder {
            store: self.store.clone(),
            inner: self.inner.clone(),
            paths: self.paths.clone(),
        }
    }

    async fn begin_receiving_snapshot(
        &mut self,
    ) -> Result<
        Box<<TypeConfig as openraft::RaftTypeConfig>::SnapshotData>,
        openraft::StorageError<NodeId>,
    > {
        Ok(Box::new(std::io::Cursor::new(Vec::new())))
    }

    async fn install_snapshot(
        &mut self,
        meta: &SnapshotMeta<NodeId, NodeMeta>,
        mut snapshot: Box<<TypeConfig as openraft::RaftTypeConfig>::SnapshotData>,
    ) -> Result<(), openraft::StorageError<NodeId>> {
        use tokio::io::{AsyncReadExt as _, AsyncSeekExt as _};

        let _ = snapshot.seek(std::io::SeekFrom::Start(0)).await;
        let mut buf = Vec::new();
        snapshot
            .read_to_end(&mut buf)
            .await
            .map_err(|e| io_err(ErrorSubject::Snapshot(None), ErrorVerb::Read, e))?;

        let raw_payload: serde_json::Value = serde_json::from_slice(&buf).map_err(|e| {
            io_err(
                ErrorSubject::Snapshot(None),
                ErrorVerb::Read,
                std::io::Error::other(e),
            )
        })?;
        let raw_state = raw_payload.get("state").cloned().ok_or_else(|| {
            io_err(
                ErrorSubject::Snapshot(None),
                ErrorVerb::Read,
                std::io::Error::other("invalid snapshot payload: missing `state` field"),
            )
        })?;
        let state = crate::state::migrate_state_value_to_latest(raw_state).map_err(|e| {
            io_err(
                ErrorSubject::Snapshot(None),
                ErrorVerb::Read,
                std::io::Error::other(e.to_string()),
            )
        })?;

        {
            let mut store = self.store.lock().await;
            *store.state_mut() = state;
            store.save().map_err(|e| {
                io_err(
                    ErrorSubject::StateMachine,
                    ErrorVerb::Write,
                    std::io::Error::other(e.to_string()),
                )
            })?;

            // Snapshot install replaces the entire state; keep local usage bounded to the
            // current memberships set to avoid stale grant/membership usage lingering.
            let allowed_membership_keys = store
                .state()
                .node_user_endpoint_memberships
                .iter()
                .map(|m| crate::state::membership_key(&m.user_id, &m.endpoint_id))
                .collect::<std::collections::BTreeSet<_>>();
            let _ = store.update_usage(|usage| {
                usage
                    .memberships
                    .retain(|key, _| allowed_membership_keys.contains(key));
            });
            let _ = store.prune_inbound_ip_usage_memberships();
        }

        {
            let mut inner = self.inner.lock().await;
            inner.last_applied = meta.last_log_id;
            inner.last_membership = meta.last_membership.clone();
        }

        self.persist_meta().await?;
        write_json(&self.paths.snapshot_meta_json, meta)
            .await
            .map_err(|e| io_err(ErrorSubject::Snapshot(None), ErrorVerb::Write, e))?;
        write_bytes(&self.paths.snapshot_data_json, &buf)
            .await
            .map_err(|e| io_err(ErrorSubject::Snapshot(None), ErrorVerb::Write, e))?;
        self.reconcile.request_full();
        Ok(())
    }

    async fn get_current_snapshot(
        &mut self,
    ) -> Result<Option<Snapshot<TypeConfig>>, openraft::StorageError<NodeId>> {
        let meta = read_json::<SnapshotMeta<NodeId, NodeMeta>>(&self.paths.snapshot_meta_json)
            .await
            .map_err(|e| io_err(ErrorSubject::Snapshot(None), ErrorVerb::Read, e))?;
        let Some(meta) = meta else {
            return Ok(None);
        };
        let bytes = read_bytes(&self.paths.snapshot_data_json)
            .await
            .map_err(|e| io_err(ErrorSubject::Snapshot(None), ErrorVerb::Read, e))?;
        Ok(Some(Snapshot {
            meta,
            snapshot: Box::new(std::io::Cursor::new(bytes)),
        }))
    }
}

fn io_err(
    subject: ErrorSubject<NodeId>,
    verb: ErrorVerb,
    err: std::io::Error,
) -> openraft::StorageError<NodeId> {
    openraft::StorageError::from_io_error(subject, verb, err)
}

async fn read_json<T: serde::de::DeserializeOwned + Send + 'static>(
    path: &Path,
) -> Result<Option<T>, std::io::Error> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        if !path.exists() {
            return Ok(None);
        }
        let bytes = std::fs::read(&path)?;
        let v = serde_json::from_slice(&bytes).map_err(std::io::Error::other)?;
        Ok(Some(v))
    })
    .await
    .expect("spawn_blocking read_json")
}

async fn read_wal_with_compat(
    path: &Path,
    last_applied_index: Option<u64>,
) -> Result<(PersistedWal, bool), std::io::Error> {
    let Some(raw_wal) = read_json::<serde_json::Value>(path).await? else {
        return Ok((PersistedWal::empty(), false));
    };

    let mut rewritten = false;
    let last_purged_log_id: Option<LogId<NodeId>> = raw_wal
        .get("last_purged_log_id")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(std::io::Error::other)?
        .unwrap_or(None);
    let last_purged_index = last_purged_log_id
        .as_ref()
        .map(|log_id| log_id.index)
        .unwrap_or(0);

    let mut entries = Vec::new();
    if let Some(raw_entries_value) = raw_wal.get("entries") {
        let Some(raw_entries) = raw_entries_value.as_array() else {
            return Err(std::io::Error::other(
                "invalid wal format: `entries` must be an array",
            ));
        };
        for raw_entry in raw_entries {
            match serde_json::from_value::<openraft::impls::Entry<TypeConfig>>(raw_entry.clone()) {
                Ok(entry) => entries.push(entry),
                Err(parse_err) => {
                    let Some(cmd_type) = extract_entry_command_type(raw_entry) else {
                        return Err(std::io::Error::other(parse_err));
                    };
                    if !is_retired_grant_group_command(&cmd_type) {
                        return Err(std::io::Error::other(parse_err));
                    }

                    let entry_index = extract_entry_log_index(raw_entry).ok_or_else(|| {
                        std::io::Error::other(format!(
                            "failed to read wal entry index for retired command: type={cmd_type}"
                        ))
                    })?;
                    let last_applied = last_applied_index.unwrap_or(0);
                    if entry_index > last_applied {
                        return Err(std::io::Error::other(format!(
                            "retired wal command is not applied yet (entry_index={entry_index}, last_applied={last_applied}); start old version to apply/snapshot first, then upgrade"
                        )));
                    }
                    if entry_index > last_purged_index {
                        return Err(std::io::Error::other(format!(
                            "retired wal command is still in active log range (entry_index={entry_index}, last_purged={last_purged_index}); start old version to snapshot/purge logs first, then upgrade"
                        )));
                    }

                    let mut blank_entry = raw_entry.clone();
                    rewrite_entry_payload_to_blank(&mut blank_entry)?;
                    let parsed =
                        serde_json::from_value::<openraft::impls::Entry<TypeConfig>>(blank_entry)
                            .map_err(std::io::Error::other)?;
                    entries.push(parsed);
                    rewritten = true;
                }
            }
        }
    }

    Ok((
        PersistedWal {
            last_purged_log_id,
            entries,
        },
        rewritten,
    ))
}

fn extract_entry_log_index(entry: &serde_json::Value) -> Option<u64> {
    entry
        .pointer("/log_id/index")
        .and_then(|value| value.as_u64())
        .or_else(|| {
            entry
                .get("log_id")
                .and_then(|log_id| log_id.get("index"))
                .and_then(|value| value.as_u64())
        })
}

fn extract_entry_command_type(entry: &serde_json::Value) -> Option<String> {
    fn find_type(value: &serde_json::Value) -> Option<String> {
        match value {
            serde_json::Value::Object(map) => {
                if let Some(serde_json::Value::String(cmd_type)) = map.get("type") {
                    return Some(cmd_type.clone());
                }
                for child in map.values() {
                    if let Some(cmd_type) = find_type(child) {
                        return Some(cmd_type);
                    }
                }
                None
            }
            serde_json::Value::Array(items) => {
                for child in items {
                    if let Some(cmd_type) = find_type(child) {
                        return Some(cmd_type);
                    }
                }
                None
            }
            _ => None,
        }
    }

    entry.get("payload").and_then(find_type)
}

fn is_retired_grant_group_command(cmd_type: &str) -> bool {
    matches!(
        cmd_type,
        "create_grant_group" | "replace_grant_group" | "delete_grant_group"
    )
}

fn rewrite_entry_payload_to_blank(entry: &mut serde_json::Value) -> Result<(), std::io::Error> {
    let blank_payload =
        serde_json::to_value(EntryPayload::<TypeConfig>::Blank).map_err(std::io::Error::other)?;
    let Some(entry_obj) = entry.as_object_mut() else {
        return Err(std::io::Error::other("wal entry is not an object"));
    };
    entry_obj.insert("payload".to_string(), blank_payload);
    Ok(())
}

async fn write_json<T: serde::Serialize>(path: &Path, value: &T) -> Result<(), std::io::Error> {
    let path = path.to_path_buf();
    let bytes = serde_json::to_vec_pretty(value).map_err(std::io::Error::other)?;
    write_bytes(&path, &bytes).await
}

async fn read_bytes(path: &Path) -> Result<Vec<u8>, std::io::Error> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || std::fs::read(&path))
        .await
        .expect("spawn_blocking read_bytes")
}

async fn write_bytes(path: &Path, bytes: &[u8]) -> Result<(), std::io::Error> {
    let path = path.to_path_buf();
    let bytes = bytes.to_vec();
    tokio::task::spawn_blocking(move || {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, &bytes)?;
        std::fs::rename(tmp, path)?;
        Ok(())
    })
    .await
    .expect("spawn_blocking write_bytes")
}

#[cfg(test)]
mod tests {
    use std::{path::Path, sync::Arc};

    use serde_json::json;
    use tokio::sync::{Mutex, mpsc};

    use super::*;
    use crate::{
        domain::{
            Endpoint, EndpointKind, Node, NodeQuotaReset, QuotaResetSource, User, UserQuotaReset,
        },
        reconcile::ReconcileRequest,
        state::{JsonSnapshotStore, StoreInit, UserNodeQuotaConfig},
    };

    fn test_store_init(tmp_dir: &Path) -> StoreInit {
        StoreInit {
            data_dir: tmp_dir.to_path_buf(),
            bootstrap_node_id: None,
            bootstrap_node_name: "node-1".to_string(),
            bootstrap_access_host: "".to_string(),
            bootstrap_api_base_url: "https://127.0.0.1:62416".to_string(),
        }
    }

    fn build_entry(cmd: DesiredStateCommand, index: u64) -> openraft::impls::Entry<TypeConfig> {
        let log_id = LogId::new(openraft::CommittedLeaderId::new(1, 1), index);
        openraft::impls::Entry {
            log_id,
            payload: EntryPayload::Normal(cmd),
        }
    }

    fn build_retired_grant_group_raw_entry(index: u64, cmd_type: &str) -> serde_json::Value {
        let log_id = LogId::new(openraft::CommittedLeaderId::new(1, 1), index);
        let mut raw_entry = serde_json::to_value(openraft::impls::Entry::<TypeConfig> {
            log_id,
            payload: EntryPayload::Blank,
        })
        .unwrap();
        raw_entry["payload"] = json!({
            "normal": {
                "type": cmd_type,
            }
        });
        raw_entry
    }

    #[tokio::test]
    async fn upsert_endpoint_change_requests_rebuild_inbound() {
        let tmp = tempfile::tempdir().unwrap();
        let (tx, mut rx) = mpsc::unbounded_channel();
        let reconcile = ReconcileHandle::from_sender(tx);
        let store = JsonSnapshotStore::load_or_init(test_store_init(tmp.path())).unwrap();
        let store = Arc::new(Mutex::new(store));
        let endpoint_id = {
            let mut store = store.lock().await;
            let node_id = store.list_nodes()[0].node_id.clone();
            let endpoint = store
                .create_endpoint(
                    node_id,
                    EndpointKind::VlessRealityVisionTcp,
                    443,
                    json!({
                        "reality": {
                            "dest": "example.com:443",
                            "server_names": ["example.com"],
                            "fingerprint": "chrome"
                        }
                    }),
                )
                .unwrap();
            endpoint.endpoint_id
        };

        let mut state_machine = FileStateMachine::open(tmp.path(), store.clone(), reconcile)
            .await
            .unwrap();

        let mut endpoint = {
            let store = store.lock().await;
            store.get_endpoint(&endpoint_id).unwrap()
        };
        endpoint.port = 8443;

        let entry = build_entry(DesiredStateCommand::UpsertEndpoint { endpoint }, 1);
        state_machine.apply(vec![entry]).await.unwrap();

        let mut requests = Vec::new();
        while let Ok(req) = rx.try_recv() {
            requests.push(req);
        }
        assert!(requests.iter().any(|req| {
            matches!(
                req,
                ReconcileRequest::RebuildInbound { endpoint_id: id } if id == &endpoint_id
            )
        }));
    }

    #[tokio::test]
    async fn install_snapshot_migrates_legacy_grants_state_to_v10() {
        let tmp = tempfile::tempdir().unwrap();
        let reconcile = ReconcileHandle::noop();
        let store = JsonSnapshotStore::load_or_init(test_store_init(tmp.path())).unwrap();
        let store = Arc::new(Mutex::new(store));
        let mut state_machine = FileStateMachine::open(tmp.path(), store.clone(), reconcile)
            .await
            .unwrap();

        let node = Node {
            node_id: "node_1".to_string(),
            node_name: "node-1".to_string(),
            access_host: "example.com".to_string(),
            api_base_url: "https://127.0.0.1:62416".to_string(),
            quota_limit_bytes: 0,
            quota_reset: NodeQuotaReset::default(),
        };
        let endpoint = Endpoint {
            endpoint_id: "endpoint_1".to_string(),
            node_id: node.node_id.clone(),
            tag: "ss".to_string(),
            kind: EndpointKind::Ss2022_2022Blake3Aes128Gcm,
            port: 8388,
            meta: json!({}),
        };
        let user = User {
            user_id: "user_1".to_string(),
            display_name: "alice".to_string(),
            subscription_token: "sub_1".to_string(),
            credential_epoch: 0,
            priority_tier: Default::default(),
            quota_reset: UserQuotaReset::default(),
        };

        let legacy_snapshot = json!({
            "state": {
                "schema_version": 9,
                "nodes": {
                    node.node_id.clone(): node,
                },
                "endpoints": {
                    endpoint.endpoint_id.clone(): endpoint,
                },
                "users": {
                    user.user_id.clone(): user,
                },
                "grants": {
                    "grant_1": {
                        "grant_id": "grant_1",
                        "user_id": "user_1",
                        "endpoint_id": "endpoint_1",
                        "enabled": true,
                    },
                },
                "user_node_quotas": {
                    "user_1": {
                        "node_1": UserNodeQuotaConfig {
                            quota_limit_bytes: Some(100 * 1024 * 1024 * 1024),
                            quota_reset_source: QuotaResetSource::User,
                        }
                    }
                }
            }
        });

        let bytes = serde_json::to_vec_pretty(&legacy_snapshot).unwrap();
        let meta = SnapshotMeta {
            last_log_id: None,
            last_membership: StoredMembership::default(),
            snapshot_id: "snapshot-test".to_string(),
        };

        state_machine
            .install_snapshot(&meta, Box::new(std::io::Cursor::new(bytes)))
            .await
            .unwrap();

        let store = store.lock().await;
        assert_eq!(store.state().schema_version, crate::state::SCHEMA_VERSION);
        assert!(store.state().user_node_quotas.is_empty());
        assert!(
            store
                .state()
                .node_user_endpoint_memberships
                .iter()
                .any(|m| m.user_id == "user_1"
                    && m.endpoint_id == "endpoint_1"
                    && m.node_id == "node_1")
        );
    }

    #[tokio::test]
    async fn read_wal_with_compat_rejects_non_array_entries() {
        let tmp = tempfile::tempdir().unwrap();
        let wal_path = tmp.path().join("wal.json");
        let raw = serde_json::to_vec(&json!({
            "last_purged_log_id": null,
            "entries": {
                "unexpected": true
            }
        }))
        .unwrap();
        std::fs::write(&wal_path, raw).unwrap();

        let err = read_wal_with_compat(&wal_path, Some(0)).await.unwrap_err();
        assert!(err.to_string().contains("entries"));
        assert!(err.to_string().contains("array"));
    }

    #[tokio::test]
    async fn read_wal_with_compat_rejects_unapplied_retired_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let wal_path = tmp.path().join("wal.json");
        let raw_entry = build_retired_grant_group_raw_entry(5, "create_grant_group");
        let last_purged_log_id =
            serde_json::to_value(LogId::new(openraft::CommittedLeaderId::new(1, 1), 5)).unwrap();
        let raw = serde_json::to_vec(&json!({
            "last_purged_log_id": last_purged_log_id,
            "entries": [raw_entry]
        }))
        .unwrap();
        std::fs::write(&wal_path, raw).unwrap();

        let err = read_wal_with_compat(&wal_path, Some(4)).await.unwrap_err();
        assert!(err.to_string().contains("not applied yet"));
    }

    #[tokio::test]
    async fn read_wal_with_compat_rejects_unpurged_retired_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let wal_path = tmp.path().join("wal.json");
        let raw_entry = build_retired_grant_group_raw_entry(5, "replace_grant_group");
        let last_purged_log_id =
            serde_json::to_value(LogId::new(openraft::CommittedLeaderId::new(1, 1), 4)).unwrap();
        let raw = serde_json::to_vec(&json!({
            "last_purged_log_id": last_purged_log_id,
            "entries": [raw_entry]
        }))
        .unwrap();
        std::fs::write(&wal_path, raw).unwrap();

        let err = read_wal_with_compat(&wal_path, Some(10)).await.unwrap_err();
        assert!(err.to_string().contains("active log range"));
    }

    #[tokio::test]
    async fn read_wal_with_compat_rewrites_purged_retired_entry_to_blank() {
        let tmp = tempfile::tempdir().unwrap();
        let wal_path = tmp.path().join("wal.json");
        let raw_entry = build_retired_grant_group_raw_entry(5, "delete_grant_group");
        let last_purged_log_id =
            serde_json::to_value(LogId::new(openraft::CommittedLeaderId::new(1, 1), 5)).unwrap();
        let raw = serde_json::to_vec(&json!({
            "last_purged_log_id": last_purged_log_id,
            "entries": [raw_entry]
        }))
        .unwrap();
        std::fs::write(&wal_path, raw).unwrap();

        let (wal, rewritten) = read_wal_with_compat(&wal_path, Some(10)).await.unwrap();
        assert!(rewritten);
        assert_eq!(wal.entries.len(), 1);
        assert!(matches!(wal.entries[0].payload, EntryPayload::Blank));
    }
}
