use std::convert::Infallible;

use anyhow::{Result, anyhow};
use bytes::Bytes;
use http_body_util::{BodyExt, StreamBody, combinators::BoxBody};
use hyper::{
    Request, Response, StatusCode, body::Frame, body::Incoming, server::conn::http2,
    service::service_fn,
};
use hyper_util::rt::{TokioExecutor, TokioIo};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    sync::mpsc,
};

use xp_proxy_core::control::UserSnapshot;
use xp_proxy_core::vless::{VlessRequestError, decode_request, is_authorized};

type ResponseBody = BoxBody<Bytes, Infallible>;

pub async fn serve(stream: TcpStream, users: Vec<UserSnapshot>, path: String) -> Result<()> {
    let io = TokioIo::new(stream);
    let service = service_fn(move |request| handle_request(request, users.clone(), path.clone()));
    http2::Builder::new(TokioExecutor::new())
        .serve_connection(io, service)
        .await
        .map_err(|error| anyhow!("HTTP/2 connection failed: {error}"))?;
    Ok(())
}

async fn handle_request(
    request: Request<Incoming>,
    users: Vec<UserSnapshot>,
    path: String,
) -> Result<Response<ResponseBody>, Infallible> {
    if request.uri().path() != path {
        return Ok(simple_response(StatusCode::NOT_FOUND, "path not found"));
    }
    let response = match open_stream(request, users).await {
        Ok(response) => response,
        Err(error) => simple_response(StatusCode::BAD_REQUEST, &error.to_string()),
    };
    Ok(response)
}

async fn open_stream(
    request: Request<Incoming>,
    users: Vec<UserSnapshot>,
) -> Result<Response<ResponseBody>> {
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
        if buffered.len() > 8192 {
            return Err(anyhow!("VLESS request header exceeds 8192 bytes"));
        }
        match decode_request(&buffered) {
            Ok(decoded) => break decoded,
            Err(VlessRequestError::Truncated) => continue,
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
    let destination = format!("{}:{}", vless.destination.0, vless.destination.1);
    let mut outbound = TcpStream::connect(destination).await?;
    let extra = &buffered[consumed..];
    if !extra.is_empty() {
        outbound.write_all(extra).await?;
    }
    let (sender, receiver) = mpsc::channel::<Result<Frame<Bytes>, Infallible>>(16);
    sender
        .send(Ok(Frame::data(Bytes::from_static(&[1, 0]))))
        .await
        .map_err(|_| anyhow!("HTTP/2 response stream closed"))?;
    tokio::spawn(relay_body(body, outbound, sender));
    let stream = tokio_stream::wrappers::ReceiverStream::new(receiver);
    let body = StreamBody::new(stream).boxed();
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/octet-stream")
        .body(body)?)
}

async fn relay_body(
    mut body: Incoming,
    outbound: TcpStream,
    sender: mpsc::Sender<Result<Frame<Bytes>, Infallible>>,
) {
    let (mut read_in, mut write_out) = outbound.into_split();
    let sender_for_downlink = sender;
    let downlink = async move {
        let mut buffer = [0u8; 16 * 1024];
        loop {
            let size = read_in.read(&mut buffer).await?;
            if size == 0 {
                break Ok::<(), anyhow::Error>(());
            }
            sender_for_downlink
                .send(Ok(Frame::data(Bytes::copy_from_slice(&buffer[..size]))))
                .await
                .map_err(|_| anyhow!("HTTP/2 response stream closed"))?;
        }
    };
    let uplink = async move {
        while let Some(frame) = body.frame().await {
            let frame = frame?;
            let data = frame
                .into_data()
                .map_err(|_| anyhow!("XHTTP request contained a non-data frame"))?;
            write_out.write_all(&data).await?;
        }
        write_out.shutdown().await?;
        Ok::<(), anyhow::Error>(())
    };
    let _ = tokio::select! {
        result = downlink => result,
        result = uplink => result,
    };
}

fn simple_response(status: StatusCode, message: &str) -> Response<ResponseBody> {
    Response::builder()
        .status(status)
        .header("content-type", "text/plain; charset=utf-8")
        .body(http_body_util::Full::new(Bytes::copy_from_slice(message.as_bytes())).boxed())
        .expect("static response is valid")
}
