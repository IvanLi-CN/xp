use std::convert::Infallible;
use std::sync::{Arc, Mutex};

use anyhow::{Result, anyhow};
use bytes::Bytes;
use http_body_util::{BodyExt, StreamBody, combinators::BoxBody};
use hyper::{
    Request, Response, StatusCode, body::Frame, body::Incoming, server::conn::http2,
    service::service_fn,
};
use hyper_util::rt::{TokioExecutor, TokioIo};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::TcpStream,
    sync::mpsc,
};

use xp_proxy_core::control::UserSnapshot;
use xp_proxy_core::vless::{VlessRequestError, decode_request, is_authorized};
use xp_proxy_core::xhttp::{StreamOneSession, StreamPermit};
use xp_proxy_core::{TrafficCounter, XmuxSettings};

type ResponseBody = BoxBody<Bytes, Infallible>;

pub async fn serve<S>(
    stream: S,
    users: Vec<UserSnapshot>,
    path: String,
    xmux: XmuxSettings,
    traffic: Arc<TrafficCounter>,
) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let io = TokioIo::new(stream);
    let session = Arc::new(Mutex::new(
        StreamOneSession::new(path.clone(), xmux).map_err(|error| anyhow!(error))?,
    ));
    let service = service_fn(move |request| {
        handle_request(
            request,
            users.clone(),
            path.clone(),
            session.clone(),
            traffic.clone(),
        )
    });
    http2::Builder::new(TokioExecutor::new())
        .initial_stream_window_size(256 * 1024)
        .initial_connection_window_size(1024 * 1024)
        .serve_connection(io, service)
        .await
        .map_err(|error| anyhow!("HTTP/2 connection failed: {error}"))?;
    Ok(())
}

async fn handle_request(
    request: Request<Incoming>,
    users: Vec<UserSnapshot>,
    path: String,
    session: Arc<Mutex<StreamOneSession>>,
    traffic: Arc<TrafficCounter>,
) -> Result<Response<ResponseBody>, Infallible> {
    let request_path = request.uri().path();
    let configured_path = path.trim_end_matches('/');
    let valid_path = request_path == path
        || request_path == format!("{configured_path}/")
        || request_path
            .strip_prefix(&format!("{configured_path}/"))
            .is_some();
    if !valid_path {
        return Ok(simple_response(StatusCode::NOT_FOUND, "path not found"));
    }
    let response = match open_stream(request, users, session, traffic).await {
        Ok(response) => response,
        Err(error) => simple_response(StatusCode::BAD_REQUEST, &error.to_string()),
    };
    Ok(response)
}

async fn open_stream(
    request: Request<Incoming>,
    users: Vec<UserSnapshot>,
    session: Arc<Mutex<StreamOneSession>>,
    traffic: Arc<TrafficCounter>,
) -> Result<Response<ResponseBody>> {
    if request.method() != hyper::Method::POST {
        return Err(anyhow!("XHTTP stream-one requires POST"));
    }
    let mut body = request.into_body();
    let mut buffered = Vec::with_capacity(256);
    let (vless, consumed) = loop {
        let frame = body
            .frame()
            .await
            .ok_or_else(|| anyhow!("empty XHTTP request body"))??;
        let data = frame
            .into_data()
            .map_err(|_| anyhow!("XHTTP request contained a non-data frame"))?;
        buffered.extend_from_slice(&data);
        match decode_request(&buffered) {
            Ok(decoded) => break decoded,
            Err(VlessRequestError::Truncated) if buffered.len() <= 8192 => continue,
            Err(VlessRequestError::Truncated) => {
                return Err(anyhow!("VLESS request header exceeds 8192 bytes"));
            }
            Err(error) => return Err(anyhow!(error)),
        }
    };
    let allowed = users
        .iter()
        .filter_map(|user| uuid::Uuid::parse_str(&user.uuid).ok())
        .collect::<Vec<_>>();
    if !is_authorized(&vless, &allowed) {
        return Err(anyhow!("unauthorized VLESS user"));
    }
    let permit = session
        .lock()
        .map_err(|_| anyhow!("XMUX session lock poisoned"))?
        .try_open_stream()
        .map_err(|error| anyhow!(error))?;
    let permit = SessionPermit { session, permit };
    let mut outbound =
        TcpStream::connect((vless.destination.0.as_str(), vless.destination.1)).await?;
    outbound.set_nodelay(true)?;
    let extra = &buffered[consumed..];
    if !extra.is_empty() {
        outbound.write_all(extra).await?;
        traffic.add_uplink(extra.len() as u64);
    }
    let (sender, receiver) = mpsc::channel::<Result<Frame<Bytes>, Infallible>>(16);
    sender
        .send(Ok(Frame::data(Bytes::from_static(&[0, 0]))))
        .await
        .map_err(|_| anyhow!("HTTP/2 response stream closed"))?;
    tokio::spawn(relay_body(body, outbound, sender, traffic, permit));
    let stream = tokio_stream::wrappers::ReceiverStream::new(receiver);
    let body = StreamBody::new(stream).boxed();
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "text/event-stream")
        .header("cache-control", "no-store")
        .header("x-accel-buffering", "no")
        .body(body)?)
}

// Own the stream admission across connect errors, cancellation and relay failure.
struct SessionPermit {
    session: Arc<Mutex<StreamOneSession>>,
    permit: StreamPermit,
}

impl Drop for SessionPermit {
    fn drop(&mut self) {
        if let Ok(mut session) = self.session.lock() {
            session.close_stream(self.permit);
        }
    }
}

async fn relay_body(
    mut body: Incoming,
    outbound: TcpStream,
    sender: mpsc::Sender<Result<Frame<Bytes>, Infallible>>,
    traffic: Arc<TrafficCounter>,
    _permit: SessionPermit,
) {
    let (mut read_in, mut write_out) = outbound.into_split();
    let sender_for_downlink = sender;
    let downlink_traffic = traffic.clone();
    let downlink = async move {
        let mut buffer = [0u8; 16 * 1024];
        loop {
            let size = read_in.read(&mut buffer).await?;
            if size == 0 {
                break Ok::<(), anyhow::Error>(());
            }
            downlink_traffic.add_downlink(size as u64);
            sender_for_downlink
                .send(Ok(Frame::data(Bytes::copy_from_slice(&buffer[..size]))))
                .await
                .map_err(|_| anyhow!("HTTP/2 response stream closed"))?;
        }
    };
    let uplink_traffic = traffic;
    let uplink = async move {
        while let Some(frame) = body.frame().await {
            let frame = frame?;
            let data = frame
                .into_data()
                .map_err(|_| anyhow!("XHTTP request contained a non-data frame"))?;
            write_out.write_all(&data).await?;
            uplink_traffic.add_uplink(data.len() as u64);
        }
        write_out.shutdown().await?;
        Ok::<(), anyhow::Error>(())
    };
    // A clean upload EOF is a TCP half-close: continue receiving the response.
    // Either direction failing cancels its peer and drops the admission guard.
    let _ = tokio::try_join!(downlink, uplink);
}

fn simple_response(status: StatusCode, message: &str) -> Response<ResponseBody> {
    Response::builder()
        .status(status)
        .header("content-type", "text/plain; charset=utf-8")
        .body(http_body_util::Full::new(Bytes::copy_from_slice(message.as_bytes())).boxed())
        .expect("static response is valid")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;
    use xp_proxy_core::vless::encode_tcp_request;

    #[tokio::test]
    async fn h2_body_reaches_tcp_destination_and_returns_response_body() {
        let echo_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let echo_addr = echo_listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = echo_listener.accept().await.unwrap();
            let mut buffer = [0u8; 32768];
            loop {
                let read = stream.read(&mut buffer).await.unwrap();
                if read == 0 {
                    break;
                }
                stream.write_all(&buffer[..read]).await.unwrap();
            }
        });

        let proxy_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy_addr = proxy_listener.local_addr().unwrap();
        let user = uuid::Uuid::new_v4();
        let users = vec![UserSnapshot {
            uuid: user.to_string(),
        }];
        tokio::spawn(async move {
            let (stream, _) = proxy_listener.accept().await.unwrap();
            serve(
                stream,
                users,
                "/xp-xhttp".into(),
                XmuxSettings::default(),
                Arc::new(TrafficCounter::default()),
            )
            .await
            .unwrap();
        });

        let client = TcpStream::connect(proxy_addr).await.unwrap();
        let (mut sender, connection) = h2::client::handshake(client).await.unwrap();
        tokio::spawn(async move {
            let _ = connection.await;
        });
        let request = Request::builder()
            .method("POST")
            .uri("http://xhttp.test/xp-xhttp")
            .body(())
            .unwrap();
        let (response, mut body) = sender.send_request(request, false).unwrap();
        let header =
            encode_tcp_request(user, (&echo_addr.ip().to_string(), echo_addr.port()), &[]).unwrap();
        let payload = vec![42u8; 16384];
        let mut request_body = header;
        request_body.extend_from_slice(&payload);
        body.send_data(Bytes::from(request_body), true).unwrap();

        let mut response = response.await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["content-type"], "text/event-stream");
        assert_eq!(response.headers()["cache-control"], "no-store");
        assert_eq!(response.headers()["x-accel-buffering"], "no");
        let mut received = Vec::new();
        while let Some(chunk) = response.body_mut().data().await {
            received.extend_from_slice(&chunk.unwrap());
        }
        assert_eq!(&received[..2], &[0, 0]);
        assert_eq!(&received[2..], payload.as_slice());
    }
}
