use futures_util::StreamExt as _;
use serde::de::DeserializeOwned;

pub(super) const MAX_INTERNAL_CAPABILITY_RESPONSE_BYTES: usize = 64 * 1024;

pub(super) async fn read_bounded_internal_json<T: DeserializeOwned>(
    response: reqwest::Response,
) -> Result<T, String> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_INTERNAL_CAPABILITY_RESPONSE_BYTES as u64)
    {
        return Err(format!(
            "internal capability response exceeds {} bytes",
            MAX_INTERNAL_CAPABILITY_RESPONSE_BYTES
        ));
    }
    let body = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        let mut body = Vec::with_capacity(
            response
                .content_length()
                .unwrap_or(0)
                .min(MAX_INTERNAL_CAPABILITY_RESPONSE_BYTES as u64) as usize,
        );
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|error| error.to_string())?;
            if body.len().saturating_add(chunk.len()) > MAX_INTERNAL_CAPABILITY_RESPONSE_BYTES {
                return Err(format!(
                    "internal capability response exceeds {} bytes",
                    MAX_INTERNAL_CAPABILITY_RESPONSE_BYTES
                ));
            }
            body.extend_from_slice(&chunk);
        }
        Ok::<_, String>(body)
    })
    .await
    .map_err(|_| "internal capability response timed out".to_string())??;
    serde_json::from_slice(&body).map_err(|error| error.to_string())
}
