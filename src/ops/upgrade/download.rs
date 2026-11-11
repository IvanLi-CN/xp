use crate::ops::util::tmp_path_next_to;
use anyhow::Context;
use futures_util::StreamExt;
use std::{fs, io::Write, path::Path, time::Duration};

pub(super) async fn download_to_path(url: &str, dest: &Path) -> anyhow::Result<()> {
    download_to_path_with_timeout(url, dest, Duration::from_secs(15 * 60)).await
}

async fn download_to_path_with_timeout(
    url: &str,
    dest: &Path,
    total_timeout: Duration,
) -> anyhow::Result<()> {
    let client = upgrade_http_client(total_timeout)?;
    let resp = client.get(url).send().await?.error_for_status()?;

    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }

    let tmp = tmp_path_next_to(dest);
    let mut file = fs::File::create(&tmp)?;
    let result = async {
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let buf = chunk?;
            file.write_all(&buf)?;
        }
        file.flush()?;
        anyhow::Ok(())
    }
    .await;
    drop(file);
    let result = result.and_then(|()| fs::rename(&tmp, dest).context("publish downloaded asset"));
    match result {
        Ok(()) => Ok(()),
        Err(error) => {
            match fs::remove_file(&tmp) {
                Ok(()) => {}
                Err(cleanup) if cleanup.kind() == std::io::ErrorKind::NotFound => {}
                Err(cleanup) => {
                    return Err(
                        error.context(format!("partial download cleanup failed: {cleanup}"))
                    );
                }
            }
            Err(error)
        }
    }
}

pub(super) fn upgrade_http_client(total_timeout: Duration) -> anyhow::Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent("xp-ops")
        .connect_timeout(Duration::from_secs(10))
        .read_timeout(Duration::from_secs(30))
        .timeout(total_timeout)
        .build()
        .context("build upgrade http client")
}

#[cfg(test)]
#[path = "download_tests.rs"]
mod tests;
