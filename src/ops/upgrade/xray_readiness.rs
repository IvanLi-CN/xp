use super::{
    Paths, SocketAddr, inbound_tag, read_xray_api_addr, read_xray_openrc_service,
    read_xray_systemd_unit,
};
use crate::ops::runtime_activation::restart_xray_service;
use std::fs;

pub(super) async fn restart_upgrade_xray(paths: &Paths) -> bool {
    if !restart_xray_service(
        paths,
        &read_xray_systemd_unit(paths),
        &read_xray_openrc_service(paths),
    ) {
        return false;
    }
    // Alternate filesystem roots use fixture service commands, not host listeners.
    if crate::ops::util::is_test_root(paths.root()) {
        return true;
    }
    let addr = read_xray_api_addr(paths).or_else(|| {
        let config = fs::read(paths.etc_xray_config()).ok()?;
        let config: serde_json::Value = serde_json::from_slice(&config).ok()?;
        let api = config["inbounds"]
            .as_array()?
            .iter()
            .find(|item| inbound_tag(item) == Some("api"))?;
        let host = api["listen"].as_str().unwrap_or("127.0.0.1");
        let port = u16::try_from(api["port"].as_u64()?).ok()?;
        Some(SocketAddr::new(host.parse().ok()?, port))
    });
    let Some(addr) = addr else {
        return false;
    };
    let addr = match addr.ip() {
        std::net::IpAddr::V4(ip) if ip.is_unspecified() => {
            SocketAddr::from(([127, 0, 0, 1], addr.port()))
        }
        std::net::IpAddr::V6(ip) if ip.is_unspecified() => {
            SocketAddr::new(std::net::Ipv6Addr::LOCALHOST.into(), addr.port())
        }
        _ => addr,
    };
    wait_for_upgrade_xray_api(
        addr,
        crate::ops::runtime_activation::service_ready_timeout(),
    )
    .await
}

async fn wait_for_upgrade_xray_api(addr: SocketAddr, timeout: std::time::Duration) -> bool {
    use crate::xray::proto::xray::app::stats::command::{
        SysStatsRequest, stats_service_client::StatsServiceClient,
    };
    use std::time::Duration;
    let attempt = Duration::from_secs(1);
    let Ok(endpoint) = tonic::transport::Endpoint::from_shared(format!("http://{addr}")) else {
        return false;
    };
    let endpoint = endpoint.connect_timeout(attempt).timeout(attempt);
    tokio::time::timeout(timeout, async {
        loop {
            if let Ok(channel) = endpoint.connect().await
                && StatsServiceClient::new(channel)
                    .get_sys_stats(SysStatsRequest {})
                    .await
                    .is_ok()
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    })
    .await
    .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn xray_api_readiness_rejects_listening_socket_without_stats_service() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let non_grpc = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            use tokio::io::AsyncWriteExt;
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
                .await
                .unwrap();
        });
        assert!(!wait_for_upgrade_xray_api(addr, Duration::from_millis(750)).await);
        non_grpc.await.unwrap();
    }
}
