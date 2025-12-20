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

        let wal = read_json::<PersistedWal>(&paths.wal_json)
            .await
            .map_err(|e| io_err(ErrorSubject::Logs, ErrorVerb::Read, e))?
            .unwrap_or_else(PersistedWal::empty);
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
                    match cmd.apply(store.state_mut()) {
                        Ok(apply_result) => {
                            store.save().map_err(|e| {
                                io_err(
                                    ErrorSubject::StateMachine,
                                    ErrorVerb::Write,
                                    std::io::Error::other(e.to_string()),
                                )
                            })?;

                            match (&cmd, &apply_result) {
                                (
                                    DesiredStateCommand::DeleteGrant { grant_id },
                                    crate::state::DesiredStateApplyResult::GrantDeleted { deleted },
                                ) => {
                                    if *deleted {
                                        store.clear_grant_usage(grant_id).map_err(|e| {
                                            io_err(
                                                ErrorSubject::StateMachine,
                                                ErrorVerb::Write,
                                                std::io::Error::other(e.to_string()),
                                            )
                                        })?;
                                    }
                                }
                                (
                                    DesiredStateCommand::UpdateGrantFields { grant_id, .. }
                                    | DesiredStateCommand::SetGrantEnabled { grant_id, .. },
                                    _,
                                ) => {
                                    store.clear_quota_banned(grant_id).map_err(|e| {
                                        io_err(
                                            ErrorSubject::StateMachine,
                                            ErrorVerb::Write,
                                            std::io::Error::other(e.to_string()),
                                        )
                                    })?;
                                }
                                _ => {}
                            }

                            ClientResponse::Ok {
                                result: apply_result,
                            }
                        }
                        Err(crate::state::StoreError::Domain(domain)) => {
                            let (status, code) = match domain {
                                crate::domain::DomainError::MissingUser { .. }
                                | crate::domain::DomainError::MissingEndpoint { .. } => {
                                    (404, "not_found")
                                }
                                _ => (400, "invalid_request"),
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

        let payload: SnapshotPayload = serde_json::from_slice(&buf).map_err(|e| {
            io_err(
                ErrorSubject::Snapshot(None),
                ErrorVerb::Read,
                std::io::Error::other(e),
            )
        })?;

        {
            let mut store = self.store.lock().await;
            *store.state_mut() = payload.state;
            store.save().map_err(|e| {
                io_err(
                    ErrorSubject::StateMachine,
                    ErrorVerb::Write,
                    std::io::Error::other(e.to_string()),
                )
            })?;
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
