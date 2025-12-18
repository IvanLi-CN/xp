use std::net::SocketAddr;

use tonic::transport::{Channel, Endpoint};

use crate::xray::proto::xray::app::proxyman::command::handler_service_client::HandlerServiceClient;
use crate::xray::proto::xray::app::proxyman::command::{
    AddInboundRequest, AddInboundResponse, AlterInboundRequest, AlterInboundResponse,
    RemoveInboundRequest, RemoveInboundResponse,
};
use crate::xray::proto::xray::app::stats::command::stats_service_client::StatsServiceClient;
use crate::xray::proto::xray::app::stats::command::{QueryStatsRequest, QueryStatsResponse};

pub mod builder;
pub mod proto;

#[derive(Debug)]
pub enum XrayError {
    Transport(tonic::transport::Error),
}

impl std::fmt::Display for XrayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Transport(err) => write!(f, "xray transport error: {err}"),
        }
    }
}

impl std::error::Error for XrayError {}

impl From<tonic::transport::Error> for XrayError {
    fn from(value: tonic::transport::Error) -> Self {
        Self::Transport(value)
    }
}

#[derive(Debug, Clone)]
pub struct XrayClient {
    handler: HandlerServiceClient<Channel>,
    stats: StatsServiceClient<Channel>,
}

pub async fn connect(addr: SocketAddr) -> Result<XrayClient, XrayError> {
    let endpoint = Endpoint::from_shared(format!("http://{addr}"))?;
    let channel = endpoint.connect().await?;
    Ok(XrayClient {
        handler: HandlerServiceClient::new(channel.clone()),
        stats: StatsServiceClient::new(channel),
    })
}

impl XrayClient {
    pub async fn add_inbound(
        &mut self,
        req: AddInboundRequest,
    ) -> Result<AddInboundResponse, tonic::Status> {
        let resp = self.handler.add_inbound(req).await?;
        Ok(resp.into_inner())
    }

    pub async fn remove_inbound(
        &mut self,
        req: RemoveInboundRequest,
    ) -> Result<RemoveInboundResponse, tonic::Status> {
        let resp = self.handler.remove_inbound(req).await?;
        Ok(resp.into_inner())
    }

    pub async fn alter_inbound(
        &mut self,
        req: AlterInboundRequest,
    ) -> Result<AlterInboundResponse, tonic::Status> {
        let resp = self.handler.alter_inbound(req).await?;
        Ok(resp.into_inner())
    }

    pub async fn query_stats(
        &mut self,
        req: QueryStatsRequest,
    ) -> Result<QueryStatsResponse, tonic::Status> {
        let resp = self.stats.query_stats(req).await?;
        Ok(resp.into_inner())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdempotencyStatus {
    AlreadyExists,
    NotFound,
    Other,
}

pub fn classify_idempotency_status(status: &tonic::Status) -> IdempotencyStatus {
    match status.code() {
        tonic::Code::AlreadyExists => return IdempotencyStatus::AlreadyExists,
        tonic::Code::NotFound => return IdempotencyStatus::NotFound,
        _ => {}
    }

    // Xray-core sometimes returns `Code::Unknown` with a nested error message
    // (e.g. "handler not found") instead of gRPC `Code::NotFound`.
    let msg = status.message().to_ascii_lowercase();
    if msg.contains("not found") {
        return IdempotencyStatus::NotFound;
    }
    if msg.contains("already exists") || msg.contains("already exist") {
        return IdempotencyStatus::AlreadyExists;
    }

    IdempotencyStatus::Other
}

pub fn is_already_exists(status: &tonic::Status) -> bool {
    classify_idempotency_status(status) == IdempotencyStatus::AlreadyExists
}

pub fn is_not_found(status: &tonic::Status) -> bool {
    classify_idempotency_status(status) == IdempotencyStatus::NotFound
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::xray::proto::xray::app::proxyman::command::handler_service_server::{
        HandlerService, HandlerServiceServer,
    };
    use crate::xray::proto::xray::app::proxyman::command::{
        AddInboundRequest, AddInboundResponse, AddOutboundRequest, AddOutboundResponse,
        AlterInboundRequest, AlterInboundResponse, AlterOutboundRequest, AlterOutboundResponse,
        GetInboundUserRequest, GetInboundUserResponse, GetInboundUsersCountResponse,
        ListInboundsRequest, ListInboundsResponse, ListOutboundsRequest, ListOutboundsResponse,
        RemoveInboundRequest, RemoveInboundResponse, RemoveOutboundRequest, RemoveOutboundResponse,
    };
    use crate::xray::proto::xray::core::InboundHandlerConfig;

    use tokio::sync::oneshot;

    #[test]
    fn classify_idempotency_status_works() {
        let already = tonic::Status::new(tonic::Code::AlreadyExists, "exists");
        let missing = tonic::Status::new(tonic::Code::NotFound, "missing");
        let missing_unknown = tonic::Status::new(tonic::Code::Unknown, "handler not found: foo");
        let already_unknown = tonic::Status::new(tonic::Code::Unknown, "something already exists");
        let other = tonic::Status::new(tonic::Code::Internal, "boom");

        assert_eq!(
            classify_idempotency_status(&already),
            IdempotencyStatus::AlreadyExists
        );
        assert_eq!(
            classify_idempotency_status(&missing),
            IdempotencyStatus::NotFound
        );
        assert_eq!(
            classify_idempotency_status(&missing_unknown),
            IdempotencyStatus::NotFound
        );
        assert_eq!(
            classify_idempotency_status(&already_unknown),
            IdempotencyStatus::AlreadyExists
        );
        assert_eq!(
            classify_idempotency_status(&other),
            IdempotencyStatus::Other
        );

        assert!(is_already_exists(&already));
        assert!(!is_already_exists(&missing));
        assert!(is_not_found(&missing));
        assert!(!is_not_found(&already));
    }

    #[derive(Debug, Default)]
    struct TestHandlerService;

    #[tonic::async_trait]
    impl HandlerService for TestHandlerService {
        async fn add_inbound(
            &self,
            request: tonic::Request<AddInboundRequest>,
        ) -> Result<tonic::Response<AddInboundResponse>, tonic::Status> {
            let req = request.into_inner();
            if req.inbound.is_none() {
                return Err(tonic::Status::invalid_argument("inbound is required"));
            }
            Ok(tonic::Response::new(AddInboundResponse {}))
        }

        async fn remove_inbound(
            &self,
            _request: tonic::Request<RemoveInboundRequest>,
        ) -> Result<tonic::Response<RemoveInboundResponse>, tonic::Status> {
            Err(tonic::Status::unimplemented("remove_inbound"))
        }

        async fn alter_inbound(
            &self,
            _request: tonic::Request<AlterInboundRequest>,
        ) -> Result<tonic::Response<AlterInboundResponse>, tonic::Status> {
            Err(tonic::Status::unimplemented("alter_inbound"))
        }

        async fn list_inbounds(
            &self,
            _request: tonic::Request<ListInboundsRequest>,
        ) -> Result<tonic::Response<ListInboundsResponse>, tonic::Status> {
            Err(tonic::Status::unimplemented("list_inbounds"))
        }

        async fn get_inbound_users(
            &self,
            _request: tonic::Request<GetInboundUserRequest>,
        ) -> Result<tonic::Response<GetInboundUserResponse>, tonic::Status> {
            Err(tonic::Status::unimplemented("get_inbound_users"))
        }

        async fn get_inbound_users_count(
            &self,
            _request: tonic::Request<GetInboundUserRequest>,
        ) -> Result<tonic::Response<GetInboundUsersCountResponse>, tonic::Status> {
            Err(tonic::Status::unimplemented("get_inbound_users_count"))
        }

        async fn add_outbound(
            &self,
            _request: tonic::Request<AddOutboundRequest>,
        ) -> Result<tonic::Response<AddOutboundResponse>, tonic::Status> {
            Err(tonic::Status::unimplemented("add_outbound"))
        }

        async fn remove_outbound(
            &self,
            _request: tonic::Request<RemoveOutboundRequest>,
        ) -> Result<tonic::Response<RemoveOutboundResponse>, tonic::Status> {
            Err(tonic::Status::unimplemented("remove_outbound"))
        }

        async fn alter_outbound(
            &self,
            _request: tonic::Request<AlterOutboundRequest>,
        ) -> Result<tonic::Response<AlterOutboundResponse>, tonic::Status> {
            Err(tonic::Status::unimplemented("alter_outbound"))
        }

        async fn list_outbounds(
            &self,
            _request: tonic::Request<ListOutboundsRequest>,
        ) -> Result<tonic::Response<ListOutboundsResponse>, tonic::Status> {
            Err(tonic::Status::unimplemented("list_outbounds"))
        }
    }

    #[tokio::test]
    async fn xray_client_can_call_add_inbound_end_to_end() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
        let incoming = tokio_stream::wrappers::TcpListenerStream::new(listener);

        let server = tonic::transport::Server::builder()
            .add_service(HandlerServiceServer::new(TestHandlerService::default()))
            .serve_with_incoming_shutdown(incoming, async {
                let _ = shutdown_rx.await;
            });

        let server_handle = tokio::spawn(server);

        let mut client = connect(addr).await.unwrap();
        let req = AddInboundRequest {
            inbound: Some(InboundHandlerConfig {
                tag: "test-inbound".to_string(),
                receiver_settings: None,
                proxy_settings: None,
            }),
        };

        client.add_inbound(req).await.unwrap();

        let _ = shutdown_tx.send(());
        let _ = server_handle.await;
    }
}
