use super::*;

pub(super) async fn install(
    state_machine: &mut FileStateMachine,
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
    let incoming_schema_version = raw_state
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or_default() as u32;
    let active_reverse_epoch = state_machine.store.lock().await.state().reverse_mesh_epoch;
    if active_reverse_epoch != 0 && incoming_schema_version < crate::state::SCHEMA_VERSION {
        return Err(io_err(
            ErrorSubject::Snapshot(None),
            ErrorVerb::Read,
            std::io::Error::other(format!(
                concat!(
                    "snapshot schema rollback is blocked after Reverse Mesh epoch ",
                    "{} was written (incoming schema ",
                    "{}, required {})"
                ),
                active_reverse_epoch,
                incoming_schema_version,
                crate::state::SCHEMA_VERSION
            )),
        ));
    }
    legacy_mesh::validate_snapshot_payload(meta, &buf)
        .map_err(|e| io_err(ErrorSubject::Snapshot(None), ErrorVerb::Read, e))?;
    let state = crate::state::migrate_state_value_to_latest(raw_state).map_err(|e| {
        io_err(
            ErrorSubject::Snapshot(None),
            ErrorVerb::Read,
            std::io::Error::other(e.to_string()),
        )
    })?;
    let persisted_buf = legacy_mesh::normalize_snapshot_payload(meta, raw_payload)
        .map_err(|e| io_err(ErrorSubject::Snapshot(None), ErrorVerb::Write, e))?;

    // Close Mesh and persist an in-progress marker before replacing the state. If the process
    // dies while either snapshot file or the state store is being replaced, restart remains
    // fail-closed until a later authenticated state apply completes.
    state_machine
        .reconcile
        .hold_mesh_gate_until_raft_state()
        .await;
    {
        let mut inner = state_machine.inner.lock().await;
        inner.mesh_state_applied = false;
        inner.snapshot_install_pending = true;
    }
    state_machine.persist_meta().await?;

    let snapshot_mesh_state_applied = legacy_mesh::snapshot_mesh_state_applied(&persisted_buf)
        .map_err(|e| io_err(ErrorSubject::Snapshot(None), ErrorVerb::Read, e))?;
    let mesh_enabled = {
        let mut store = state_machine.store.lock().await;
        let resource_revision = store.state().mihomo_resource_revision.wrapping_add(1);
        *store.state_mut() = state;
        store.state_mut().mihomo_resource_revision = resource_revision;
        let mesh_enabled = store.state().mesh_enabled;
        store.save().map_err(|e| {
            io_err(
                ErrorSubject::StateMachine,
                ErrorVerb::Write,
                std::io::Error::other(e.to_string()),
            )
        })?;
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
        mesh_enabled
    };
    {
        let mut inner = state_machine.inner.lock().await;
        inner.last_applied = meta.last_log_id;
        inner.last_membership = meta.last_membership.clone();
        inner.mesh_state_applied = snapshot_mesh_state_applied;
    }
    write_bytes(&state_machine.paths.snapshot_data_json, &persisted_buf)
        .await
        .map_err(|e| io_err(ErrorSubject::Snapshot(None), ErrorVerb::Write, e))?;
    write_json(&state_machine.paths.snapshot_meta_json, meta)
        .await
        .map_err(|e| io_err(ErrorSubject::Snapshot(None), ErrorVerb::Write, e))?;
    state_machine.inner.lock().await.snapshot_install_pending = false;
    state_machine.persist_meta().await?;
    if snapshot_mesh_state_applied {
        state_machine.reconcile.note_mesh_state_applied();
        state_machine
            .reconcile
            .initialize_mesh_gate(mesh_enabled)
            .await;
    }
    state_machine.reconcile.request_full();
    Ok(())
}
