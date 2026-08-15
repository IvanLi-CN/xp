//! Future repository waves build on the local historical-storage boundary.

pub(crate) mod control;
pub(crate) mod identity;
pub(crate) mod query;
pub(crate) mod replica;

pub(crate) use crate::state::history_storage::{
    HistoryStorage, INBOUND_IP_USAGE_KEY, MESH_TELEMETRY_KEY, NODE_HISTORY_KEY, STATE_KEY,
    TCP_CONNECTION_USAGE_KEY, USAGE_KEY,
};
