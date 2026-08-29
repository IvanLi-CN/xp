//! Linux-only smoke coverage for the nft primitives required by ingress-guard.
//!
//! The full listener/cgroup admission test runs in the privileged shared testbox. This local
//! test deliberately skips when the host lacks Linux, nft, or NET_ADMIN rather than mutating the
//! developer workstation's firewall.

use std::io::Write;
use std::process::{Command, Stdio};

#[test]
fn ingress_guard_namespace() {
    if !cfg!(target_os = "linux") {
        eprintln!("skipping ingress guard namespace test: Linux is required");
        return;
    }
    if !std::path::Path::new("/sys/fs/cgroup/cgroup.controllers").exists() {
        eprintln!("skipping ingress guard namespace test: cgroup v2 is unavailable");
        return;
    }

    let cgroup = std::fs::read_to_string("/proc/self/cgroup")
        .ok()
        .and_then(|raw| {
            raw.lines()
                .find_map(|line| line.strip_prefix("0::"))
                .map(str::trim)
                .filter(|path| !path.is_empty())
                .map(str::to_owned)
        });
    let Some(cgroup) = cgroup else {
        eprintln!("skipping ingress guard namespace test: process is in the cgroup root");
        return;
    };
    let level = cgroup.split('/').count();
    let selector = format!(
        "socket cgroupv2 level {level} \"{cgroup}\" iifname != \"lo\" \
         tcp flags & (syn | ack | rst) == syn counter return"
    );
    let program = format!(
        r#"table inet xp_ingress_guard_namespace {{
  chain input {{
    type filter hook input priority -300; policy accept;
    {selector}
  }}
}}
"#
    );
    let mut child = match Command::new("nft")
        .args(["--check", "-f", "-"])
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            eprintln!("skipping ingress guard namespace test: nft unavailable ({error})");
            return;
        }
    };
    child
        .stdin
        .take()
        .expect("nft stdin")
        .write_all(program.as_bytes())
        .expect("write nft probe");
    let output = child.wait_with_output().expect("wait for nft probe");
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("Operation not permitted") || stderr.contains("Permission denied") {
            eprintln!("skipping ingress guard namespace test: host lacks NET_ADMIN ({stderr})");
            return;
        }
        panic!("nft ingress guard probe failed: {stderr}");
    }
}
