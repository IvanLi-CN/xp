use std::{
    collections::{BTreeMap, VecDeque},
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
};

use chrono::{DateTime, Duration, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

const LEDGER_SCHEMA_VERSION: u32 = 1;
const RETENTION: Duration = Duration::minutes(10);
pub const MAX_ENTRIES: usize = 16_384;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredResult {
    pub status: u16,
    pub body: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LedgerEntry {
    state: LedgerState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
enum LedgerState {
    Pending {
        started_at: String,
    },
    Complete {
        completed_at: String,
        result: StoredResult,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedLedger {
    schema_version: u32,
    #[serde(default)]
    entries: BTreeMap<String, LedgerEntry>,
    #[serde(default)]
    order: VecDeque<String>,
}

impl Default for PersistedLedger {
    fn default() -> Self {
        Self {
            schema_version: LEDGER_SCHEMA_VERSION,
            entries: BTreeMap::new(),
            order: VecDeque::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BeginResult {
    New,
    Existing(StoredResult),
    InFlight,
    Full,
}

#[derive(Clone)]
pub struct InternalIdempotencyLedger {
    path: Arc<PathBuf>,
    state: Arc<Mutex<PersistedLedger>>,
}

impl InternalIdempotencyLedger {
    pub fn load(data_dir: &Path) -> anyhow::Result<Self> {
        let path = data_dir.join("mesh").join("idempotency.json");
        let mut persisted = if path.exists() {
            serde_json::from_slice::<PersistedLedger>(&fs::read(&path)?)?
        } else {
            PersistedLedger::default()
        };
        if persisted.schema_version != LEDGER_SCHEMA_VERSION {
            anyhow::bail!(
                "internal idempotency schema_version mismatch: expected {}, got {}",
                LEDGER_SCHEMA_VERSION,
                persisted.schema_version
            );
        }
        prune(&mut persisted, Utc::now());
        Ok(Self {
            path: Arc::new(path),
            state: Arc::new(Mutex::new(persisted)),
        })
    }

    pub async fn begin(&self, request_id: &str) -> anyhow::Result<BeginResult> {
        if request_id.trim().is_empty() || request_id.len() > 256 {
            anyhow::bail!("request_id is invalid");
        }
        let mut state = self.state.lock().await;
        let mut next = state.clone();
        prune(&mut next, Utc::now());
        if let Some(entry) = next.entries.get(request_id) {
            return Ok(match &entry.state {
                LedgerState::Pending { .. } => BeginResult::InFlight,
                LedgerState::Complete { result, .. } => BeginResult::Existing(result.clone()),
            });
        }
        if next.entries.len() >= MAX_ENTRIES {
            // Do not evict still-valid records: ambiguous retries must always receive their first
            // answer, so capacity pressure is a deliberate rejection rather than silent loss.
            return Ok(BeginResult::Full);
        }
        next.entries.insert(
            request_id.to_string(),
            LedgerEntry {
                state: LedgerState::Pending {
                    started_at: timestamp(Utc::now()),
                },
            },
        );
        next.order.push_back(request_id.to_string());
        persist(&self.path, &next)?;
        *state = next;
        Ok(BeginResult::New)
    }

    pub async fn finish(&self, request_id: &str, result: StoredResult) -> anyhow::Result<()> {
        let mut state = self.state.lock().await;
        let mut next = state.clone();
        prune(&mut next, Utc::now());
        if !next.entries.contains_key(request_id) && next.entries.len() >= MAX_ENTRIES {
            anyhow::bail!("idempotency ledger is full");
        }
        next.entries.insert(
            request_id.to_string(),
            LedgerEntry {
                state: LedgerState::Complete {
                    completed_at: timestamp(Utc::now()),
                    result,
                },
            },
        );
        if !next.order.iter().any(|id| id == request_id) {
            next.order.push_back(request_id.to_string());
        }
        persist(&self.path, &next)?;
        *state = next;
        Ok(())
    }
}

fn prune(state: &mut PersistedLedger, now: DateTime<Utc>) {
    while let Some(id) = state.order.front().cloned() {
        let expired = state
            .entries
            .get(&id)
            .and_then(|entry| match &entry.state {
                LedgerState::Pending { started_at } => {
                    DateTime::parse_from_rfc3339(started_at).ok()
                }
                LedgerState::Complete { completed_at, .. } => {
                    DateTime::parse_from_rfc3339(completed_at).ok()
                }
            })
            .is_some_and(|at| now.signed_duration_since(at.with_timezone(&Utc)) > RETENTION);
        if !expired {
            break;
        }
        state.order.pop_front();
        state.entries.remove(&id);
    }
}

fn persist(path: &Path, value: &PersistedLedger) -> anyhow::Result<()> {
    let parent = path.parent().expect("ledger path has a parent");
    fs::create_dir_all(parent)?;
    let temporary = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec(value)?;
    {
        let mut file = fs::File::create(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
    }
    fs::rename(temporary, path)?;
    Ok(())
}

fn timestamp(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Secs, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn returns_first_result_after_restart() {
        let temp = tempfile::tempdir().unwrap();
        let ledger = InternalIdempotencyLedger::load(temp.path()).unwrap();
        assert_eq!(ledger.begin("request-1").await.unwrap(), BeginResult::New);
        ledger
            .finish(
                "request-1",
                StoredResult {
                    status: 201,
                    body: serde_json::json!({ "created": true }),
                },
            )
            .await
            .unwrap();
        let restored = InternalIdempotencyLedger::load(temp.path()).unwrap();
        assert!(matches!(
            restored.begin("request-1").await.unwrap(),
            BeginResult::Existing(StoredResult { status: 201, .. })
        ));
    }

    #[tokio::test]
    async fn pending_request_never_executes_twice_after_restart() {
        let temp = tempfile::tempdir().unwrap();
        let ledger = InternalIdempotencyLedger::load(temp.path()).unwrap();
        assert_eq!(ledger.begin("request-1").await.unwrap(), BeginResult::New);
        assert_eq!(
            ledger.begin("request-1").await.unwrap(),
            BeginResult::InFlight
        );

        let restored = InternalIdempotencyLedger::load(temp.path()).unwrap();
        assert_eq!(
            restored.begin("request-1").await.unwrap(),
            BeginResult::InFlight
        );
    }

    fn ledger_with_unwritable_parent(
        temp: &tempfile::TempDir,
        state: PersistedLedger,
    ) -> InternalIdempotencyLedger {
        let parent = temp.path().join("not-a-directory");
        fs::write(&parent, b"file blocks ledger directory").unwrap();
        InternalIdempotencyLedger {
            path: Arc::new(parent.join("idempotency.json")),
            state: Arc::new(Mutex::new(state)),
        }
    }

    #[tokio::test]
    async fn failed_begin_does_not_publish_an_unpersisted_pending_entry() {
        let temp = tempfile::tempdir().unwrap();
        let ledger = ledger_with_unwritable_parent(&temp, PersistedLedger::default());

        assert!(ledger.begin("request-1").await.is_err());

        let state = ledger.state.lock().await;
        assert!(state.entries.is_empty());
        assert!(state.order.is_empty());
    }

    #[tokio::test]
    async fn failed_finish_keeps_the_persisted_pending_state_in_memory() {
        let temp = tempfile::tempdir().unwrap();
        let mut initial = PersistedLedger::default();
        initial.entries.insert(
            "request-1".to_string(),
            LedgerEntry {
                state: LedgerState::Pending {
                    started_at: timestamp(Utc::now()),
                },
            },
        );
        initial.order.push_back("request-1".to_string());
        let ledger = ledger_with_unwritable_parent(&temp, initial);

        assert!(
            ledger
                .finish(
                    "request-1",
                    StoredResult {
                        status: 200,
                        body: serde_json::json!({ "ok": true }),
                    },
                )
                .await
                .is_err()
        );

        let state = ledger.state.lock().await;
        assert!(matches!(
            state.entries["request-1"].state,
            LedgerState::Pending { .. }
        ));
    }
}
