use crate::{
    domain::Node, join_session::JoinSession, raft::app::RaftFacade, state::DesiredStateCommand,
};
use std::sync::Arc;

pub(super) async fn tombstone_non_replayable_legacy_session(
    raft: &Arc<dyn RaftFacade>,
    session: JoinSession,
    node: Option<Node>,
) -> anyhow::Result<()> {
    // Keep the blocked operation as audit evidence, but make the malformed reservation terminal
    // so the coordinator does not emit the same warning and retry forever.
    let mut terminal_session = session;
    terminal_session.status = crate::join_session::JoinSessionStatus::Expired;
    terminal_session.terminal_at = Some(chrono::Utc::now().to_rfc3339());
    let command = match node {
        Some(node) => DesiredStateCommand::UpsertNode {
            node,
            join_session: Some(terminal_session),
        },
        None => DesiredStateCommand::DeleteNode {
            node_id: terminal_session.node_id.clone(),
            delete_endpoints: false,
            expected_endpoint_ids: Vec::new(),
            join_session: Some(terminal_session),
        },
    };
    super::write_applied(raft, command).await?;
    Ok(())
}
