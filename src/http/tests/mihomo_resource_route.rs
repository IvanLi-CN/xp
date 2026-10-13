use super::*;

use crate::{mihomo_resources, subscription};
use pretty_assertions::assert_eq;

#[tokio::test(start_paused = true)]
async fn deadline_includes_store_lock() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, store) = app_with(&tmp, ReconcileHandle::noop());
    let cluster = ClusterMetadata::load(tmp.path()).unwrap();
    let key = cluster
        .read_cluster_ca_key_pem(tmp.path())
        .unwrap()
        .unwrap();
    let url = subscription::normalize_mihomo_external_url(mihomo_resources::FIXED_GEOX_ASSETS[0].1)
        .unwrap();
    let resource_id = mihomo_resources::resource_id(&key, &url);
    let _guard = store.lock().await;
    let request =
        tokio::spawn(app.oneshot(req("GET", &format!("/api/mihomo/resources/{resource_id}"))));
    tokio::task::yield_now().await;
    tokio::time::advance(std::time::Duration::from_secs(90)).await;
    let response = request.await.unwrap().unwrap();
    assert_eq!(response.status(), StatusCode::GATEWAY_TIMEOUT);
}
