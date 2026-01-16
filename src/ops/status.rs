use crate::ops::cli::{ExitError, StatusArgs};
use crate::ops::paths::Paths;
use crate::ops::platform::{Distro, detect_distro};
use crate::ops::util::is_executable;
use serde::Serialize;
use std::fs;
use std::path::Path;
use std::process::Command;

pub async fn cmd_status(paths: Paths, args: StatusArgs) -> Result<(), ExitError> {
    let distro = detect_distro(&paths).ok();
    let cloudflared_abs = match distro {
        Some(Distro::Alpine) => Path::new("/usr/local/bin/cloudflared"),
        _ => Path::new("/usr/bin/cloudflared"),
    };

    let xp = bin_status(&paths, Path::new("/usr/local/bin/xp"), &["--version"]);
    let xray = bin_status(&paths, Path::new("/usr/local/bin/xray"), &["version"]);
    let cloudflared = bin_status(&paths, cloudflared_abs, &["--version"]);

    let xp_work_dir = dir_status(&paths, Path::new("/var/lib/xp"));
    let xp_data_dir = dir_status(&paths, Path::new("/var/lib/xp/data"));

    if args.json {
        let out = StatusJson {
            xp,
            cloudflared,
            xray,
            xp_work_dir,
            xp_data_dir,
        };
        let s = serde_json::to_string_pretty(&out)
            .map_err(|e| ExitError::new(2, format!("invalid_args: {e}")))?;
        println!("{s}");
        return Ok(());
    }

    println!(
        "xp: {} ({})",
        if xp.present { "present" } else { "missing" },
        xp.path.as_deref().unwrap_or("-")
    );
    println!(
        "xray: {} ({})",
        if xray.present { "present" } else { "missing" },
        xray.path.as_deref().unwrap_or("-")
    );
    println!(
        "cloudflared: {} ({})",
        if cloudflared.present {
            "present"
        } else {
            "missing"
        },
        cloudflared.path.as_deref().unwrap_or("-")
    );
    println!(
        "xp_work_dir: {}",
        xp_work_dir.path.as_deref().unwrap_or("-")
    );
    println!(
        "xp_data_dir: {}",
        xp_data_dir.path.as_deref().unwrap_or("-")
    );

    Ok(())
}

#[derive(Debug, Serialize)]
struct StatusJson {
    xp: BinInfo,
    cloudflared: BinInfo,
    xray: BinInfo,
    xp_work_dir: DirInfo,
    xp_data_dir: DirInfo,
}

#[derive(Debug, Serialize)]
struct BinInfo {
    present: bool,
    version: Option<String>,
    path: Option<String>,
}

#[derive(Debug, Serialize)]
struct DirInfo {
    path: Option<String>,
    exists: bool,
    owner: Option<String>,
    group: Option<String>,
}

fn bin_status(paths: &Paths, abs: &Path, version_args: &[&str]) -> BinInfo {
    let mapped = paths.map_abs(abs);
    let present = mapped.exists() && is_executable(&mapped);
    let path = if present {
        Some(abs.display().to_string())
    } else {
        None
    };

    let version = if present && paths.root() == Path::new("/") {
        Command::new(abs)
            .args(version_args)
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    } else {
        None
    };

    BinInfo {
        present,
        version,
        path,
    }
}

fn dir_status(paths: &Paths, abs: &Path) -> DirInfo {
    let mapped = paths.map_abs(abs);
    let exists = mapped.exists();
    let (owner, group) = if exists {
        // Best-effort: platform-dependent; keep None if not available.
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let md = fs::metadata(&mapped).ok();
            if let Some(md) = md {
                let uid = md.uid();
                let gid = md.gid();
                return DirInfo {
                    path: Some(abs.display().to_string()),
                    exists,
                    owner: Some(uid.to_string()),
                    group: Some(gid.to_string()),
                };
            }
        }
        (None, None)
    } else {
        (None, None)
    };

    DirInfo {
        path: Some(abs.display().to_string()),
        exists,
        owner,
        group,
    }
}
