use crate::ops::cli::{DeployRuntimeArg, ExitError};
use crate::ops::paths::Paths;
use crate::ops::runtime::RuntimeTarget;
use crate::ops::util::{chmod, ensure_dir, write_string_if_changed};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

const DEPLOY_SELECTION_VERSION: u32 = 1;

/// Where a deploy's ingress runtime choice came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimeSelectionSource {
    PersistedState,
    PersistedDeploySelection,
    ExistingDeploymentLegacyXray,
    FreshDefault,
    Explicit,
}

impl RuntimeSelectionSource {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::PersistedState => "persisted local runtime state",
            Self::PersistedDeploySelection => "persisted deploy runtime selection",
            Self::ExistingDeploymentLegacyXray => {
                "existing deployment without runtime state (legacy xray)"
            }
            Self::FreshDefault => "fresh deployment default",
            Self::Explicit => "explicit --runtime",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RuntimeSelection {
    pub target: RuntimeTarget,
    pub source: RuntimeSelectionSource,
}

/// Durable record of the runtime chosen by the deploy that created this node.
///
/// It exists only so a retried deploy can tell a genuinely fresh node (already mutated by the
/// failed attempt) from a legacy deployment. The runtime state file stays authoritative and is
/// written only once services are actually active.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct DeploySelectionRecord {
    version: u32,
    runtime: RuntimeTarget,
}

/// Resolves the runtime a deploy must use.
///
/// Precedence: persisted runtime state, then the durable fresh-deploy selection, then the legacy
/// implicit Xray choice for an existing deployment, then the fresh-deploy default (`xp-proxy`).
pub(crate) fn resolve_runtime_selection(
    requested: DeployRuntimeArg,
    persisted_state: Option<RuntimeTarget>,
    deploy_selection: Option<RuntimeTarget>,
    existing_deployment: bool,
) -> Result<RuntimeSelection, ExitError> {
    let preserved = persisted_state
        .map(|target| (target, RuntimeSelectionSource::PersistedState))
        .or_else(|| {
            deploy_selection
                .map(|target| (target, RuntimeSelectionSource::PersistedDeploySelection))
        })
        .or_else(|| {
            existing_deployment.then_some((
                RuntimeTarget::Xray,
                RuntimeSelectionSource::ExistingDeploymentLegacyXray,
            ))
        });
    let explicit = match requested {
        DeployRuntimeArg::Auto => None,
        DeployRuntimeArg::Xray => Some(RuntimeTarget::Xray),
        DeployRuntimeArg::XpProxy => Some(RuntimeTarget::XpProxy),
    };

    match (preserved, explicit) {
        (Some((target, source)), None) => Ok(RuntimeSelection { target, source }),
        (Some((target, _)), Some(requested)) if requested == target => Ok(RuntimeSelection {
            target,
            source: RuntimeSelectionSource::Explicit,
        }),
        (Some((target, _)), Some(requested)) => Err(ExitError::new(
            2,
            format!(
                concat!(
                    "runtime_conflict: node ingress runtime is {}; ",
                    "use `xp-ops runtime switch --to {} --yes` to change it"
                ),
                target.as_str(),
                requested.as_str()
            ),
        )),
        (None, Some(target)) => Ok(RuntimeSelection {
            target,
            source: RuntimeSelectionSource::Explicit,
        }),
        (None, None) => Ok(RuntimeSelection {
            target: RuntimeTarget::XpProxy,
            source: RuntimeSelectionSource::FreshDefault,
        }),
    }
}

pub(crate) fn resolve_deploy_runtime(
    paths: &Paths,
    requested: DeployRuntimeArg,
) -> Result<RuntimeSelection, ExitError> {
    resolve_runtime_selection(
        requested,
        crate::ops::runtime::persisted_state_runtime(paths)?,
        load_deploy_selection(paths)?,
        existing_deployment_present(paths),
    )
}

/// A node counts as an existing deployment only after XP env or cluster identity exists.
///
/// `xp-ops init` alone creates `/etc/xp` and the data directory, so those are not evidence of a
/// previously selected runtime.
pub(crate) fn existing_deployment_present(paths: &Paths) -> bool {
    if paths.etc_xp_env().exists() {
        return true;
    }
    let cluster =
        crate::cluster_metadata::ClusterPaths::new(&paths.map_abs(Path::new("/var/lib/xp/data")));
    cluster.metadata_json.exists() || cluster.admin_token_hash.exists()
}

pub(crate) fn load_deploy_selection(paths: &Paths) -> Result<Option<RuntimeTarget>, ExitError> {
    match fs::read(paths.etc_xp_ops_deploy_runtime_selection()) {
        Ok(bytes) => {
            let record: DeploySelectionRecord = serde_json::from_slice(&bytes)
                .map_err(|error| ExitError::new(5, format!("deploy_selection_invalid: {error}")))?;
            if record.version != DEPLOY_SELECTION_VERSION {
                return Err(ExitError::new(
                    5,
                    format!("deploy_selection_unsupported: version {}", record.version),
                ));
            }
            Ok(Some(record.runtime))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(ExitError::new(
            5,
            format!("deploy_selection_read_failed: {error}"),
        )),
    }
}

/// Persists the fresh-deploy runtime choice before any join/env mutation.
///
/// An existing record is never overwritten: a retried deploy must resolve the original choice.
pub(crate) fn persist_deploy_selection(
    paths: &Paths,
    target: RuntimeTarget,
) -> Result<bool, ExitError> {
    if load_deploy_selection(paths)?.is_some() {
        return Ok(false);
    }
    ensure_dir(&paths.etc_xp_ops_deploy_dir())
        .map_err(|error| ExitError::new(5, format!("deploy_selection_write_failed: {error}")))?;
    let record = DeploySelectionRecord {
        version: DEPLOY_SELECTION_VERSION,
        runtime: target,
    };
    let content = serde_json::to_string_pretty(&record)
        .map_err(|error| ExitError::new(5, format!("deploy_selection_encode_failed: {error}")))?;
    let path = paths.etc_xp_ops_deploy_runtime_selection();
    write_string_if_changed(&path, &(content + "\n"))
        .map_err(|error| ExitError::new(5, format!("deploy_selection_write_failed: {error}")))?;
    chmod(&path, 0o644).ok();
    Ok(true)
}

/// Drops the deploy-time record once an explicit runtime switch has persisted an active state.
pub(crate) fn remove_deploy_selection(paths: &Paths) {
    let _ = fs::remove_file(paths.etc_xp_ops_deploy_runtime_selection());
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn fresh_auto_selects_xp_proxy() {
        let selection =
            resolve_runtime_selection(DeployRuntimeArg::Auto, None, None, false).unwrap();
        assert_eq!(selection.target, RuntimeTarget::XpProxy);
        assert_eq!(selection.source, RuntimeSelectionSource::FreshDefault);
    }

    #[test]
    fn fresh_explicit_xray_is_allowed() {
        let selection =
            resolve_runtime_selection(DeployRuntimeArg::Xray, None, None, false).unwrap();
        assert_eq!(selection.target, RuntimeTarget::Xray);
        assert_eq!(selection.source, RuntimeSelectionSource::Explicit);
    }

    #[test]
    fn existing_xray_state_is_preserved() {
        let selection = resolve_runtime_selection(
            DeployRuntimeArg::Auto,
            Some(RuntimeTarget::Xray),
            None,
            true,
        )
        .unwrap();
        assert_eq!(selection.target, RuntimeTarget::Xray);
        assert_eq!(selection.source, RuntimeSelectionSource::PersistedState);
    }

    #[test]
    fn existing_xp_proxy_state_is_preserved() {
        let selection = resolve_runtime_selection(
            DeployRuntimeArg::Auto,
            Some(RuntimeTarget::XpProxy),
            None,
            true,
        )
        .unwrap();
        assert_eq!(selection.target, RuntimeTarget::XpProxy);
        assert_eq!(selection.source, RuntimeSelectionSource::PersistedState);
    }

    #[test]
    fn existing_legacy_deployment_without_state_stays_xray() {
        let selection =
            resolve_runtime_selection(DeployRuntimeArg::Auto, None, None, true).unwrap();
        assert_eq!(selection.target, RuntimeTarget::Xray);
        assert_eq!(
            selection.source,
            RuntimeSelectionSource::ExistingDeploymentLegacyXray
        );
    }

    #[test]
    fn failed_fresh_deploy_retry_keeps_the_persisted_xp_proxy_choice() {
        let selection = resolve_runtime_selection(
            DeployRuntimeArg::Auto,
            None,
            Some(RuntimeTarget::XpProxy),
            true,
        )
        .unwrap();
        assert_eq!(selection.target, RuntimeTarget::XpProxy);
        assert_eq!(
            selection.source,
            RuntimeSelectionSource::PersistedDeploySelection
        );
    }

    #[test]
    fn persisted_state_wins_over_the_deploy_selection() {
        let selection = resolve_runtime_selection(
            DeployRuntimeArg::Auto,
            Some(RuntimeTarget::Xray),
            Some(RuntimeTarget::XpProxy),
            true,
        )
        .unwrap();
        assert_eq!(selection.target, RuntimeTarget::Xray);
        assert_eq!(selection.source, RuntimeSelectionSource::PersistedState);
    }

    #[test]
    fn explicit_matching_runtime_on_existing_node_is_allowed() {
        let selection = resolve_runtime_selection(
            DeployRuntimeArg::XpProxy,
            Some(RuntimeTarget::XpProxy),
            None,
            true,
        )
        .unwrap();
        assert_eq!(selection.target, RuntimeTarget::XpProxy);
        assert_eq!(selection.source, RuntimeSelectionSource::Explicit);
    }

    #[test]
    fn explicit_conflicting_runtime_on_existing_node_fails_closed() {
        let error = resolve_runtime_selection(
            DeployRuntimeArg::XpProxy,
            Some(RuntimeTarget::Xray),
            None,
            true,
        )
        .expect_err("must not implicitly switch an existing node");

        assert_eq!(error.code, 2);
        assert!(error.message.starts_with("runtime_conflict:"));
        assert!(
            error
                .message
                .contains("xp-ops runtime switch --to xp-proxy --yes")
        );
    }

    #[test]
    fn explicit_conflict_against_a_legacy_node_fails_closed() {
        let error = resolve_runtime_selection(DeployRuntimeArg::XpProxy, None, None, true)
            .expect_err("a legacy node must not be switched implicitly");
        assert!(error.message.contains("switch --to xp-proxy --yes"));
    }

    #[test]
    fn deploy_selection_round_trips_without_overwriting() {
        let dir = tempdir().unwrap();
        let paths = Paths::new(dir.path().to_path_buf());

        assert_eq!(
            persist_deploy_selection(&paths, RuntimeTarget::XpProxy).unwrap(),
            true
        );
        assert_eq!(
            persist_deploy_selection(&paths, RuntimeTarget::Xray).unwrap(),
            false
        );
        assert_eq!(
            load_deploy_selection(&paths).unwrap(),
            Some(RuntimeTarget::XpProxy)
        );

        let raw = fs::read_to_string(paths.etc_xp_ops_deploy_runtime_selection()).unwrap();
        assert!(raw.contains("\"version\": 1"));
        assert!(raw.contains("\"runtime\": \"xp-proxy\""));

        remove_deploy_selection(&paths);
        assert_eq!(load_deploy_selection(&paths).unwrap(), None);
    }

    #[test]
    fn invalid_deploy_selection_fails_closed() {
        let dir = tempdir().unwrap();
        let paths = Paths::new(dir.path().to_path_buf());
        fs::create_dir_all(paths.etc_xp_ops_deploy_dir()).unwrap();
        fs::write(paths.etc_xp_ops_deploy_runtime_selection(), "{not json").unwrap();

        let error = load_deploy_selection(&paths).expect_err("invalid marker must fail");
        assert!(error.message.starts_with("deploy_selection_invalid:"));
    }

    #[test]
    fn xp_env_or_cluster_identity_marks_an_existing_deployment() {
        let dir = tempdir().unwrap();
        let paths = Paths::new(dir.path().to_path_buf());
        assert!(!existing_deployment_present(&paths));

        fs::create_dir_all(paths.etc_xp_ops_deploy_dir()).unwrap();
        assert!(!existing_deployment_present(&paths));

        fs::create_dir_all(paths.etc_xp_dir()).unwrap();
        fs::write(paths.etc_xp_env(), "XP_DATA_DIR=/var/lib/xp/data\n").unwrap();
        assert!(existing_deployment_present(&paths));
    }
}
