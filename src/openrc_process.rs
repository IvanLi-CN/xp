use std::{collections::BTreeSet, path::PathBuf, time::Duration};

use tokio::process::Command;
use tracing::warn;

const OPENRC_KILL_SUPERVISOR_HELPER: &str = "/usr/local/libexec/xp-openrc-kill-supervisor";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OpenrcProcess {
    pub pid: u32,
    pub command: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct OpenrcProcessAudit {
    pub supervisors: Vec<OpenrcProcess>,
    pub workers: Vec<OpenrcProcess>,
}

pub(crate) async fn audit_and_cleanup_duplicates(
    service: &str,
    worker_commands: &[&str],
    timeout: Duration,
    phase: &'static str,
    cleanup: bool,
) {
    let audit = match audit_openrc_processes(service, worker_commands, timeout).await {
        Ok(audit) => audit,
        Err(err) => {
            warn!(
                service,
                phase,
                error = %err,
                "openrc process audit failed"
            );
            return;
        }
    };

    if audit.supervisors.len() > 1 {
        warn!(
            service,
            phase,
            count = audit.supervisors.len(),
            pids = ?audit.supervisors.iter().map(|p| p.pid).collect::<Vec<_>>(),
            "multiple openrc supervise-daemon instances detected"
        );
        if cleanup {
            terminate_duplicate_supervisors(service, phase, &audit.supervisors, timeout).await;
        }
    }

    if audit.workers.len() > 1 {
        warn!(
            service,
            phase,
            count = audit.workers.len(),
            pids = ?audit.workers.iter().map(|p| p.pid).collect::<Vec<_>>(),
            "multiple openrc worker processes detected"
        );
        // Worker argv alone cannot prove OpenRC service ownership when operators run
        // another instance from the same executable, so xp only reports worker duplicates.
    }
}

async fn audit_openrc_processes(
    service: &str,
    worker_commands: &[&str],
    timeout: Duration,
) -> Result<OpenrcProcessAudit, String> {
    let processes = capture_processes(timeout).await?;
    Ok(classify_openrc_processes(
        &processes,
        service,
        worker_commands,
    ))
}

async fn capture_processes(timeout: Duration) -> Result<Vec<OpenrcProcess>, String> {
    let mut cmd = Command::new("/bin/sh");
    cmd.arg("-c")
        .arg("ps -eo pid=,args= 2>/dev/null || ps w 2>/dev/null");
    cmd.stdin(std::process::Stdio::null());
    cmd.stderr(std::process::Stdio::null());

    let output = match tokio::time::timeout(timeout, cmd.output()).await {
        Ok(Ok(output)) => output,
        Ok(Err(err)) => return Err(format!("spawn ps: {err}")),
        Err(_) => return Err("timeout running ps".to_string()),
    };

    if !output.status.success() {
        return Err(format!("ps exited with {}", output.status));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(parse_ps_output(&stdout))
}

fn classify_openrc_processes(
    processes: &[OpenrcProcess],
    service: &str,
    worker_commands: &[&str],
) -> OpenrcProcessAudit {
    let mut audit = OpenrcProcessAudit::default();

    for process in processes {
        if is_supervise_daemon_for_service(&process.command, service) {
            audit.supervisors.push(process.clone());
        } else if worker_commands
            .iter()
            .any(|command| command_has_executable_token(&process.command, command))
        {
            audit.workers.push(process.clone());
        }
    }

    audit.supervisors.sort_by_key(|process| process.pid);
    audit.workers.sort_by_key(|process| process.pid);
    audit
}

fn parse_ps_output(output: &str) -> Vec<OpenrcProcess> {
    output
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            let (pid_raw, command) = trimmed.split_once(char::is_whitespace)?;
            let pid = pid_raw.parse::<u32>().ok()?;
            let command = normalize_ps_command(command.trim());
            if command.is_empty() {
                return None;
            }
            Some(OpenrcProcess {
                pid,
                command,
            })
        })
        .collect()
}

fn is_supervise_daemon_for_service(command: &str, service: &str) -> bool {
    let tokens = command.split_whitespace().collect::<Vec<_>>();
    tokens.windows(2).any(|window| {
        let token = window[0].trim_matches(['"', '\'']);
        let service_token = window[1].trim_matches(['"', '\'']);
        command_token_basename_matches(token, "supervise-daemon") && service_token == service
    })
}

fn normalize_ps_command(command: &str) -> String {
    let tokens = command.split_whitespace().collect::<Vec<_>>();
    for (idx, token) in tokens.iter().enumerate() {
        let token = token.trim_matches(['"', '\'']);
        if token.starts_with('/') || command_token_basename_matches(token, "supervise-daemon") {
            return tokens[idx..].join(" ");
        }
    }
    command.to_string()
}

fn command_token_basename_matches(token: &str, needle: &str) -> bool {
    token == needle || token.rsplit('/').next() == Some(needle)
}

fn command_has_executable_token(command: &str, needle: &str) -> bool {
    let Some(token) = command.split_whitespace().next() else {
        return false;
    };
    let token = token.trim_matches(['"', '\'']);
    token == needle
}

async fn terminate_duplicate_supervisors(
    service: &str,
    phase: &'static str,
    processes: &[OpenrcProcess],
    timeout: Duration,
) {
    let Some(active_pid) = active_supervisor_pid(service, processes) else {
        warn!(
            service,
            phase,
            pids = ?processes.iter().map(|p| p.pid).collect::<Vec<_>>(),
            "skipped duplicate openrc supervisor cleanup because active supervisor pid is unknown"
        );
        return;
    };

    for process in processes.iter().filter(|process| process.pid != active_pid) {
        if let Err(err) = terminate_supervisor_process(service, process.pid, timeout).await {
            warn!(
                service,
                phase,
                kind = "supervisor",
                pid = process.pid,
                error = %err,
                "failed to terminate duplicate openrc process"
            );
        } else {
            warn!(
                service,
                phase,
                kind = "supervisor",
                pid = process.pid,
                kept_pid = active_pid,
                "terminated duplicate openrc process"
            );
        }
    }
}

fn active_supervisor_pid(service: &str, processes: &[OpenrcProcess]) -> Option<u32> {
    let known_pids = processes
        .iter()
        .map(|process| process.pid)
        .collect::<BTreeSet<_>>();
    let mut candidates = Vec::new();
    for process in processes {
        candidates.extend(supervisor_pidfile_paths(&process.command));
    }
    candidates.push(PathBuf::from(format!("/run/supervise-{service}.pid")));
    candidates.push(PathBuf::from(format!("/var/run/supervise-{service}.pid")));

    candidates
        .into_iter()
        .filter_map(|path| read_pidfile(&path))
        .find(|pid| known_pids.contains(pid))
}

fn supervisor_pidfile_paths(command: &str) -> Vec<PathBuf> {
    let tokens = command.split_whitespace().collect::<Vec<_>>();
    let mut paths = Vec::new();
    for (idx, token) in tokens.iter().enumerate() {
        if let Some(path) = token.strip_prefix("--pidfile=") {
            paths.push(PathBuf::from(path));
        } else if (*token == "--pidfile" || *token == "-p")
            && let Some(path) = tokens.get(idx + 1)
        {
            paths.push(PathBuf::from(path));
        }
    }
    paths
}

fn read_pidfile(path: &PathBuf) -> Option<u32> {
    std::fs::read_to_string(path)
        .ok()?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

async fn terminate_supervisor_process(
    service: &str,
    pid: u32,
    timeout: Duration,
) -> Result<(), String> {
    let pid = pid.to_string();
    let direct_args = ["-TERM", pid.as_str()];
    match run_command_with_timeout(&["/bin/kill", "/usr/bin/kill", "kill"], &direct_args, timeout)
        .await
    {
        Ok(()) => Ok(()),
        Err(err) => {
            let mut privileged_errors = Vec::new();
            let doas_args = ["-n", OPENRC_KILL_SUPERVISOR_HELPER, service, pid.as_str()];
            match run_command_with_timeout(
                &["/usr/bin/doas", "/bin/doas", "doas"],
                &doas_args,
                timeout,
            )
            .await
            {
                Ok(()) => return Ok(()),
                Err(privileged_err) => privileged_errors.push(format!("doas: {privileged_err}")),
            }

            let sudo_args = ["-n", OPENRC_KILL_SUPERVISOR_HELPER, service, pid.as_str()];
            match run_command_with_timeout(
                &["/usr/bin/sudo", "/bin/sudo", "sudo"],
                &sudo_args,
                timeout,
            )
            .await
            {
                Ok(()) => return Ok(()),
                Err(privileged_err) => privileged_errors.push(format!("sudo: {privileged_err}")),
            }

            Err(format!(
                "direct kill failed: {err}; privileged kill failed: {}",
                privileged_errors.join("; ")
            ))
        }
    }
}

async fn run_command_with_timeout(
    programs: &[&str],
    args: &[&str],
    timeout: Duration,
) -> Result<(), String> {
    for program in programs {
        let mut cmd = Command::new(program);
        cmd.args(args);
        cmd.stdin(std::process::Stdio::null());
        cmd.stdout(std::process::Stdio::null());
        cmd.stderr(std::process::Stdio::null());

        let status = match tokio::time::timeout(timeout, cmd.status()).await {
            Ok(Ok(status)) => status,
            Ok(Err(err)) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Ok(Err(err)) => return Err(format!("spawn {program}: {err}")),
            Err(_) => return Err(format!("timeout running {program}")),
        };

        if status.success() {
            return Ok(());
        }
        return Err(format!("{program} exited with {status}"));
    }

    Err("no matching program found".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_only_matching_openrc_service_and_workers() {
        let processes = parse_ps_output(
            r#"
              101 supervise-daemon xray --start -- /usr/local/bin/xray run -c /etc/xray/config.json
              102 supervise-daemon cloudflared --start -- /usr/local/bin/cloudflared tunnel run
              103 /usr/local/bin/xray run -c /etc/xray/config.json
              104 /usr/local/bin/cloudflared tunnel run
              105 /bin/sh -c echo xray
              106 xray run -c /tmp/manual-config.json
            "#,
        );

        let audit = classify_openrc_processes(&processes, "xray", &["/usr/local/bin/xray"]);

        assert_eq!(audit.supervisors.len(), 1);
        assert_eq!(audit.supervisors[0].pid, 101);
        assert_eq!(audit.workers.len(), 1);
        assert_eq!(audit.workers[0].pid, 103);
    }

    #[test]
    fn supervise_daemon_service_match_ignores_worker_path_basename() {
        let processes = parse_ps_output(
            r#"
              101 supervise-daemon other --start -- /usr/local/bin/xray run -c /etc/other/config.json
              102 supervise-daemon xray --start -- /usr/local/bin/xray run -c /etc/xray/config.json
            "#,
        );

        let audit = classify_openrc_processes(&processes, "xray", &["/usr/local/bin/xray"]);

        assert_eq!(audit.supervisors.len(), 1);
        assert_eq!(audit.supervisors[0].pid, 102);
    }

    #[test]
    fn normalizes_busybox_ps_w_columns_before_worker_matching() {
        let processes = parse_ps_output(
            r#"
              PID   USER     TIME  COMMAND
              101   root     0:00  supervise-daemon xray --start -- /usr/local/bin/xray run
              102   root     0:01  /usr/local/bin/xray run -c /etc/xray/config.json
              103   root     0:01  /usr/local/bin/xray run -c /etc/xray/config.json
            "#,
        );

        let audit = classify_openrc_processes(&processes, "xray", &["/usr/local/bin/xray"]);

        assert_eq!(audit.supervisors.len(), 1);
        assert_eq!(audit.supervisors[0].pid, 101);
        assert_eq!(
            audit
                .workers
                .iter()
                .map(|process| process.pid)
                .collect::<Vec<_>>(),
            vec![102, 103]
        );
    }

    #[test]
    fn keeps_duplicate_processes_sorted_for_oldest_cleanup() {
        let processes = parse_ps_output(
            r#"
              301 /usr/local/bin/xray run -c /etc/xray/config.json
              201 supervise-daemon xray --start -- /usr/local/bin/xray run -c /etc/xray/config.json
              401 /usr/local/bin/xray run -c /etc/xray/config.json
              101 supervise-daemon xray --start -- /usr/local/bin/xray run -c /etc/xray/config.json
            "#,
        );

        let audit = classify_openrc_processes(&processes, "xray", &["/usr/local/bin/xray"]);

        assert_eq!(
            audit
                .supervisors
                .iter()
                .map(|process| process.pid)
                .collect::<Vec<_>>(),
            vec![101, 201]
        );
        assert_eq!(
            audit
                .workers
                .iter()
                .map(|process| process.pid)
                .collect::<Vec<_>>(),
            vec![301, 401]
        );
    }

    #[test]
    fn extracts_supervisor_pidfile_paths() {
        let paths = supervisor_pidfile_paths(
            "supervise-daemon xray --pidfile /run/custom.pid --start -- /usr/local/bin/xray run",
        );
        assert_eq!(paths, vec![PathBuf::from("/run/custom.pid")]);

        let paths = supervisor_pidfile_paths(
            "supervise-daemon xray --pidfile=/run/inline.pid --start -- /usr/local/bin/xray run",
        );
        assert_eq!(paths, vec![PathBuf::from("/run/inline.pid")]);
    }

    #[test]
    fn active_supervisor_pid_uses_pidfile_evidence() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), "201\n").unwrap();
        let output = format!(
            "101 supervise-daemon xray --pidfile {} --start -- /usr/local/bin/xray run\n\
             201 supervise-daemon xray --pidfile {} --start -- /usr/local/bin/xray run\n",
            tmp.path().display(),
            tmp.path().display()
        );
        let processes = parse_ps_output(&output);

        assert_eq!(active_supervisor_pid("xray", &processes), Some(201));
    }

    #[test]
    fn active_supervisor_pid_ignores_pidfile_for_unknown_process() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), "999\n").unwrap();
        let output = format!(
            "101 supervise-daemon xray --pidfile {} --start -- /usr/local/bin/xray run\n\
             201 supervise-daemon xray --pidfile {} --start -- /usr/local/bin/xray run\n",
            tmp.path().display(),
            tmp.path().display()
        );
        let processes = parse_ps_output(&output);

        assert_eq!(active_supervisor_pid("xray", &processes), None);
    }
}
