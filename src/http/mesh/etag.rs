use sha2::{Digest, Sha256};

use super::AdminMeshStatusResponse;

pub(super) fn mesh_status_etag(snapshot: &AdminMeshStatusResponse) -> String {
    let mut stable_snapshot = snapshot.clone();
    stable_snapshot.generated_at.clear();
    if let Some(usage) = stable_snapshot.local.connection_usage.as_mut() {
        usage.sampled_at = None;
    }
    let stable_bytes = serde_json::to_vec(&stable_snapshot).expect("serialize mesh status ETag");
    format!("\"mesh-{}\"", hex::encode(Sha256::digest(stable_bytes)))
}
