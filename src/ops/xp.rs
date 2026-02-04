use crate::ops::cli::{
    ExitError, XpBootstrapArgs, XpInstallArgs, XpRestartArgs, XpSyncNodeMetaArgs,
};
use crate::ops::paths::Paths;
use crate::ops::util::{Mode, chmod, ensure_dir, is_test_root, write_bytes_if_changed};
use axum::http::{Method, Uri, header::HeaderName};
use std::fs;
use std::path::Path;
use std::process::Command;

pub async fn cmd_xp_install(paths: Paths, args: XpInstallArgs) -> Result<(), ExitError> {
    let mode = if args.dry_run {
        Mode::DryRun
    } else {
        Mode::Real
    };

    if mode == Mode::DryRun {
        eprintln!("would copy: {} -> /usr/local/bin/xp", args.xp_bin.display());
        if args.enable {
            eprintln!("would enable and start xp service (init-system auto)");
        }
        return Ok(());
    }

    let src = args.xp_bin;
    if !src.exists() {
        return Err(ExitError::new(2, "invalid_args: --xp-bin does not exist"));
    }

    let dest = paths.usr_local_bin_xp();
    if let Some(parent) = dest.parent() {
        ensure_dir(parent).map_err(|e| ExitError::new(3, format!("filesystem_error: {e}")))?;
    }

    let bytes = fs::read(&src).map_err(|e| ExitError::new(3, format!("filesystem_error: {e}")))?;
    write_bytes_if_changed(&dest, &bytes)
        .map_err(|e| ExitError::new(3, format!("filesystem_error: {e}")))?;
    chmod(&dest, 0o755).ok();

    if !is_test_root(paths.root()) {
        let status = Command::new("/usr/local/bin/xp")
            .arg("--help")
            .status()
            .map_err(|e| ExitError::new(3, format!("filesystem_error: xp verify: {e}")))?;
        if !status.success() {
            return Err(ExitError::new(3, "filesystem_error: xp verify failed"));
        }
    }

    if args.enable && !is_test_root(paths.root()) {
        // Defer to init-system auto behavior: try systemd first, then OpenRC.
        if Command::new("systemctl")
            .args(["enable", "--now", "xp.service"])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
        {
            return Ok(());
        }
        let _ = Command::new("rc-update")
            .args(["add", "xp", "default"])
            .status();
        let _ = Command::new("rc-service").args(["xp", "start"]).status();
    }

    Ok(())
}

pub async fn cmd_xp_bootstrap(paths: Paths, args: XpBootstrapArgs) -> Result<(), ExitError> {
    let mode = if args.dry_run {
        Mode::DryRun
    } else {
        Mode::Real
    };

    validate_https_origin(&args.api_base_url)?;

    let xp_bin = paths.map_abs(Path::new("/usr/local/bin/xp"));
    if !xp_bin.exists() {
        return Err(ExitError::new(3, "xp_not_installed"));
    }

    let metadata_path = paths
        .map_abs(&args.xp_data_dir)
        .join("cluster")
        .join("metadata.json");
    if metadata_path.exists() {
        return Ok(());
    }

    if mode == Mode::DryRun {
        eprintln!("would run as user xp: /usr/local/bin/xp init ...");
        return Ok(());
    }

    if is_test_root(paths.root()) {
        return Err(ExitError::new(
            5,
            "xp_init_failed: xp bootstrap requires real system environment (use --dry-run for tests)",
        ));
    }

    // Prefer runuser if present; fallback to su.
    let has_runuser = Command::new("runuser")
        .arg("--help")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    let status = if has_runuser {
        let mut c = Command::new("runuser");
        c.args(["-u", "xp", "--", "/usr/local/bin/xp", "init"]);
        c.args([
            "--data-dir",
            args.xp_data_dir.to_string_lossy().as_ref(),
            "--node-name",
            &args.node_name,
            "--access-host",
            &args.access_host,
            "--api-base-url",
            &args.api_base_url,
        ]);
        c.status()
    } else {
        let cmdline = format!(
            "/usr/local/bin/xp init --data-dir {} --node-name {} --access-host {} --api-base-url {}",
            sh_quote(&args.xp_data_dir.to_string_lossy()),
            sh_quote(&args.node_name),
            sh_quote(&args.access_host),
            sh_quote(&args.api_base_url),
        );
        Command::new("su")
            .args(["-s", "/bin/sh", "xp", "-c", &cmdline])
            .status()
    };
    let status = status.map_err(|e| ExitError::new(5, format!("xp_init_failed: {e}")))?;
    if !status.success() {
        return Err(ExitError::new(5, "xp_init_failed"));
    }
    Ok(())
}

pub async fn cmd_xp_restart(paths: Paths, args: XpRestartArgs) -> Result<(), ExitError> {
    if args.dry_run {
        eprintln!(
            "would restart xp service (init-system auto): {}",
            args.service_name
        );
        return Ok(());
    }

    if is_test_root(paths.root()) {
        return Err(ExitError::new(
            5,
            "xp_restart_failed: xp restart requires real system environment (use --dry-run for tests)",
        ));
    }

    let service = args.service_name.as_str();

    // Prefer init-system auto behavior: try systemd first, then OpenRC.
    let systemd_ok = Command::new("systemctl")
        .args(["restart", format!("{service}.service").as_str()])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if systemd_ok {
        return Ok(());
    }

    let openrc_ok = Command::new("rc-service")
        .args([service, "restart"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if openrc_ok {
        return Ok(());
    }

    Err(ExitError::new(
        6,
        "xp_restart_failed: failed to restart service (hint: run via sudo; ensure systemctl/rc-service exists)",
    ))
}

pub async fn cmd_xp_sync_node_meta(
    paths: Paths,
    args: XpSyncNodeMetaArgs,
) -> Result<(), ExitError> {
    let mode = if args.dry_run {
        Mode::DryRun
    } else {
        Mode::Real
    };

    let env_path = paths.etc_xp_env();
    let raw_env = fs::read_to_string(&env_path)
        .map_err(|_| ExitError::new(2, "invalid_input: /etc/xp/xp.env not found"))?;
    let parsed = crate::ops::xp_env::parse_xp_env(Some(raw_env));

    let node_name = parsed.node_name.ok_or_else(|| {
        ExitError::new(2, "invalid_input: XP_NODE_NAME missing in /etc/xp/xp.env")
    })?;
    let access_host = parsed.access_host.ok_or_else(|| {
        ExitError::new(2, "invalid_input: XP_ACCESS_HOST missing in /etc/xp/xp.env")
    })?;
    let api_base_url = parsed.api_base_url.ok_or_else(|| {
        ExitError::new(
            2,
            "invalid_input: XP_API_BASE_URL missing in /etc/xp/xp.env",
        )
    })?;

    if node_name.trim().is_empty() {
        return Err(ExitError::new(2, "invalid_input: XP_NODE_NAME is empty"));
    }
    validate_https_origin(&api_base_url)?;

    let data_dir = parsed
        .data_dir
        .unwrap_or_else(|| "/var/lib/xp/data".to_string());
    if data_dir.trim().is_empty() {
        return Err(ExitError::new(2, "invalid_input: XP_DATA_DIR is empty"));
    }
    let abs_data_dir = paths.map_abs(Path::new(&data_dir));

    let mut meta = crate::cluster_metadata::ClusterMetadata::load(&abs_data_dir)
        .map_err(|e| ExitError::new(5, format!("cluster_metadata_error: {e}")))?;
    let node_id = meta.node_id.clone();

    let ca_key_pem = meta
        .read_cluster_ca_key_pem(&abs_data_dir)
        .map_err(|e| ExitError::new(5, format!("cluster_ca_key_error: {e}")))?
        .ok_or_else(|| ExitError::new(5, "cluster_ca_key_missing"))?;

    let cluster_ca_pem = meta
        .read_cluster_ca_pem(&abs_data_dir)
        .map_err(|e| ExitError::new(5, format!("cluster_ca_error: {e}")))?;
    let client = build_xp_ops_http_client(&args.xp_base_url, &cluster_ca_pem)?;

    let current_node =
        fetch_admin_node_internal(&client, &args.xp_base_url, &ca_key_pem, &node_id).await?;

    let current = current_node.clone().unwrap_or_else(|| crate::domain::Node {
        node_id: node_id.clone(),
        node_name: meta.node_name.clone(),
        access_host: meta.access_host.clone(),
        api_base_url: meta.api_base_url.clone(),
        quota_reset: crate::domain::NodeQuotaReset::default(),
    });

    eprintln!("xp node meta sync:");
    eprintln!("  node_id: {node_id}");
    eprintln!("  desired:");
    eprintln!("    node_name: {node_name}");
    eprintln!("    access_host: {access_host}");
    eprintln!("    api_base_url: {api_base_url}");
    eprintln!("  current (raft state machine):");
    eprintln!("    node_name: {}", current.node_name);
    eprintln!("    access_host: {}", current.access_host);
    eprintln!("    api_base_url: {}", current.api_base_url);

    if mode == Mode::DryRun {
        eprintln!("dry-run: no changes applied");
        return Ok(());
    }

    // 1) Update local persisted cluster metadata to match config file.
    meta.node_name = node_name.clone();
    meta.access_host = access_host.clone();
    meta.api_base_url = api_base_url.clone();
    meta.save(&abs_data_dir)
        .map_err(|e| ExitError::new(5, format!("cluster_metadata_error: {e}")))?;

    // 2) Update Raft state machine node record (used by subscription output and admin UI).
    let desired_node = crate::domain::Node {
        node_id: node_id.clone(),
        node_name: node_name.clone(),
        access_host: access_host.clone(),
        api_base_url: api_base_url.clone(),
        quota_reset: current.quota_reset.clone(),
    };
    internal_client_write(
        &client,
        &args.xp_base_url,
        &ca_key_pem,
        crate::state::DesiredStateCommand::UpsertNode { node: desired_node },
    )
    .await?;

    // 3) Update Raft membership NodeMeta (used for leader discovery and forwarding).
    let info = fetch_cluster_info(&client, &args.xp_base_url).await?;
    let set_nodes_base_url = if info.role == "leader" {
        args.xp_base_url.as_str()
    } else {
        info.leader_api_base_url.as_str()
    };
    if set_nodes_base_url.trim().is_empty() {
        return Err(ExitError::new(
            5,
            "cluster_error: leader_api_base_url is empty",
        ));
    }
    internal_set_nodes(
        &client,
        set_nodes_base_url,
        &ca_key_pem,
        vec![InternalSetNodeArgs {
            node_id: node_id.clone(),
            node_name,
            api_base_url,
        }],
    )
    .await?;

    Ok(())
}

pub async fn cmd_xp_join(
    paths: Paths,
    xp_data_dir: std::path::PathBuf,
    node_name: String,
    access_host: String,
    api_base_url: String,
    join_token: String,
    dry_run: bool,
) -> Result<(), ExitError> {
    let mode = if dry_run { Mode::DryRun } else { Mode::Real };

    if join_token.trim().is_empty() {
        return Err(ExitError::new(2, "invalid_args: join token is empty"));
    }
    validate_https_origin(&api_base_url)?;

    let xp_bin = paths.map_abs(Path::new("/usr/local/bin/xp"));
    if !xp_bin.exists() {
        return Err(ExitError::new(3, "xp_not_installed"));
    }

    let metadata_path = paths
        .map_abs(&xp_data_dir)
        .join("cluster")
        .join("metadata.json");
    if metadata_path.exists() {
        return Ok(());
    }

    if mode == Mode::DryRun {
        eprintln!("would run as user xp: /usr/local/bin/xp join ...");
        return Ok(());
    }

    if is_test_root(paths.root()) {
        return Err(ExitError::new(
            5,
            "xp_join_failed: xp join requires real system environment (use --dry-run for tests)",
        ));
    }

    // Prefer runuser if present; fallback to su.
    let has_runuser = Command::new("runuser")
        .arg("--help")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    let status = if has_runuser {
        let mut c = Command::new("runuser");
        c.args(["-u", "xp", "--", "/usr/local/bin/xp", "join"]);
        c.args([
            "--data-dir",
            xp_data_dir.to_string_lossy().as_ref(),
            "--node-name",
            &node_name,
            "--access-host",
            &access_host,
            "--api-base-url",
            &api_base_url,
            "--token",
            &join_token,
        ]);
        c.status()
    } else {
        let cmdline = format!(
            "/usr/local/bin/xp join --data-dir {} --node-name {} --access-host {} --api-base-url {} --token {}",
            sh_quote(&xp_data_dir.to_string_lossy()),
            sh_quote(&node_name),
            sh_quote(&access_host),
            sh_quote(&api_base_url),
            sh_quote(&join_token),
        );
        Command::new("su")
            .args(["-s", "/bin/sh", "xp", "-c", &cmdline])
            .status()
    };
    let status = status.map_err(|e| ExitError::new(5, format!("xp_join_failed: {e}")))?;
    if !status.success() {
        return Err(ExitError::new(5, "xp_join_failed"));
    }
    Ok(())
}

fn sh_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for ch in s.chars() {
        if ch == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

fn validate_https_origin(origin: &str) -> Result<(), ExitError> {
    let url = reqwest::Url::parse(origin)
        .map_err(|_| ExitError::new(2, "invalid_args: --api-base-url must be a valid URL"))?;
    if url.scheme() != "https" {
        return Err(ExitError::new(
            2,
            "invalid_args: --api-base-url must use https",
        ));
    }
    if url.path() != "/" || url.query().is_some() || url.fragment().is_some() {
        return Err(ExitError::new(
            2,
            "invalid_args: --api-base-url must be an origin (no path/query)",
        ));
    }
    Ok(())
}

fn validate_origin(origin: &str) -> Result<(), ExitError> {
    let url = reqwest::Url::parse(origin)
        .map_err(|_| ExitError::new(2, "invalid_args: --xp-base-url must be a valid URL"))?;
    if url.scheme() != "http" && url.scheme() != "https" {
        return Err(ExitError::new(
            2,
            "invalid_args: --xp-base-url must use http or https",
        ));
    }
    if url.path() != "/" || url.query().is_some() || url.fragment().is_some() {
        return Err(ExitError::new(
            2,
            "invalid_args: --xp-base-url must be an origin (no path/query)",
        ));
    }
    Ok(())
}

fn build_xp_ops_http_client(
    base_url: &str,
    cluster_ca_pem: &str,
) -> Result<reqwest::Client, ExitError> {
    validate_origin(base_url)?;
    let ca = reqwest::Certificate::from_pem(cluster_ca_pem.as_bytes())
        .map_err(|e| ExitError::new(2, format!("invalid_input: cluster ca cert: {e}")))?;
    reqwest::Client::builder()
        .add_root_certificate(ca)
        .build()
        .map_err(|e| ExitError::new(5, format!("http_error: build client: {e}")))
}

#[derive(serde::Deserialize)]
struct ClusterInfoPartial {
    role: String,
    leader_api_base_url: String,
}

async fn fetch_cluster_info(
    client: &reqwest::Client,
    base_url: &str,
) -> Result<ClusterInfoPartial, ExitError> {
    let base = base_url.trim_end_matches('/');
    let url = format!("{base}/api/cluster/info");
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| ExitError::new(5, format!("http_error: {e}")))?;
    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        let body = resp.text().await.unwrap_or_default();
        return Err(ExitError::new(
            5,
            format!("cluster_error: cluster info failed: {status}: {body}"),
        ));
    }
    resp.json::<ClusterInfoPartial>()
        .await
        .map_err(|e| ExitError::new(5, format!("http_error: parse cluster info: {e}")))
}

async fn fetch_admin_node_internal(
    client: &reqwest::Client,
    base_url: &str,
    cluster_ca_key_pem: &str,
    node_id: &str,
) -> Result<Option<crate::domain::Node>, ExitError> {
    let base = base_url.trim_end_matches('/');
    let url = format!("{base}/api/admin/nodes/{node_id}");
    let uri: Uri = format!("/nodes/{node_id}")
        .parse()
        .map_err(|e| ExitError::new(2, format!("invalid_input: uri: {e}")))?;
    let sig = crate::internal_auth::sign_request(cluster_ca_key_pem, &Method::GET, &uri)
        .map_err(|e| ExitError::new(5, format!("sign internal request: {e}")))?;

    let resp = client
        .get(url)
        .header(
            HeaderName::from_static(crate::internal_auth::INTERNAL_SIGNATURE_HEADER),
            sig,
        )
        .send()
        .await
        .map_err(|e| ExitError::new(5, format!("http_error: {e}")))?;

    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        let body = resp.text().await.unwrap_or_default();
        return Err(ExitError::new(
            5,
            format!("http_error: admin node get failed: {status}: {body}"),
        ));
    }

    let node = resp
        .json::<crate::domain::Node>()
        .await
        .map_err(|e| ExitError::new(5, format!("http_error: parse admin node: {e}")))?;
    Ok(Some(node))
}

async fn internal_client_write(
    client: &reqwest::Client,
    base_url: &str,
    cluster_ca_key_pem: &str,
    cmd: crate::state::DesiredStateCommand,
) -> Result<(), ExitError> {
    let base = base_url.trim_end_matches('/');
    let url = format!("{base}/api/admin/_internal/raft/client-write");
    let uri: Uri = "/_internal/raft/client-write".parse().expect("valid uri");
    let sig = crate::internal_auth::sign_request(cluster_ca_key_pem, &Method::POST, &uri)
        .map_err(|e| ExitError::new(5, format!("sign internal request: {e}")))?;

    let resp = client
        .post(url)
        .header(
            HeaderName::from_static(crate::internal_auth::INTERNAL_SIGNATURE_HEADER),
            sig,
        )
        .json(&cmd)
        .send()
        .await
        .map_err(|e| ExitError::new(5, format!("http_error: {e}")))?;

    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        let body = resp.text().await.unwrap_or_default();
        return Err(ExitError::new(
            5,
            format!("http_error: client-write failed: {status}: {body}"),
        ));
    }

    let res = resp
        .json::<crate::raft::types::ClientResponse>()
        .await
        .map_err(|e| ExitError::new(5, format!("http_error: parse client-write: {e}")))?;
    match res {
        crate::raft::types::ClientResponse::Ok { .. } => Ok(()),
        crate::raft::types::ClientResponse::Err {
            status,
            code,
            message,
        } => Err(ExitError::new(
            5,
            format!("raft_error: {status} {code}: {message}"),
        )),
    }
}

#[derive(serde::Serialize)]
struct InternalSetNodesRequestArgs {
    nodes: Vec<InternalSetNodeArgs>,
}

#[derive(serde::Serialize)]
struct InternalSetNodeArgs {
    node_id: String,
    node_name: String,
    api_base_url: String,
}

async fn internal_set_nodes(
    client: &reqwest::Client,
    base_url: &str,
    cluster_ca_key_pem: &str,
    nodes: Vec<InternalSetNodeArgs>,
) -> Result<(), ExitError> {
    let base = base_url.trim_end_matches('/');
    let url = format!("{base}/api/admin/_internal/raft/set-nodes");
    let uri: Uri = "/_internal/raft/set-nodes".parse().expect("valid uri");
    let sig = crate::internal_auth::sign_request(cluster_ca_key_pem, &Method::POST, &uri)
        .map_err(|e| ExitError::new(5, format!("sign internal request: {e}")))?;

    let resp = client
        .post(url)
        .header(
            HeaderName::from_static(crate::internal_auth::INTERNAL_SIGNATURE_HEADER),
            sig,
        )
        .json(&InternalSetNodesRequestArgs { nodes })
        .send()
        .await
        .map_err(|e| ExitError::new(5, format!("http_error: {e}")))?;

    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        let body = resp.text().await.unwrap_or_default();
        return Err(ExitError::new(
            5,
            format!("http_error: set-nodes failed: {status}: {body}"),
        ));
    }
    Ok(())
}
