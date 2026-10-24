use super::GuardMode;
use std::path::{Path, PathBuf};

const XRAY_SERVICE_MARKER: &str = "# Managed by xp-ops ingress-guard service boundary\n";

pub(crate) fn render_systemd_xray_unit(xray_work_dir: &Path, mode: Option<GuardMode>) -> String {
    let (pre, exec, boundary) = match mode {
        Some(GuardMode::Enforced) => (
            concat!(
                "PermissionsStartOnly=true\n",
                "ExecStartPre=-/usr/local/bin/xp-ops _ingress-guard-prepare\n",
            ),
            "/usr/local/bin/xp-ops _ingress-guard-exec",
            "RestartPreventExitStatus=77\n",
        ),
        Some(GuardMode::Observe) => (
            concat!(
                "PermissionsStartOnly=true\n",
                "ExecStartPre=-/usr/local/bin/xp-ops _ingress-guard-prepare\n",
            ),
            "/usr/local/bin/xray run -c /etc/xray/config.json",
            "",
        ),
        None => ("", "/usr/local/bin/xray run -c /etc/xray/config.json", ""),
    };
    let marker = XRAY_SERVICE_MARKER;
    format!(
        "[Unit]\n\
Description=xray (local proxy runtime)\n\
Wants=network-online.target\n\
After=network-online.target\n\
\n\
[Service]\n\
Type=simple\n\
User=xray\n\
Group=xray\n\
WorkingDirectory={}\n\
Environment=GOMEMLIMIT={}\n\
Environment=GOGC={}\n\
{}{}ExecStart={}\n\
Restart=always\n\
RestartSec=2s\n\
{}\n\
[Install]\n\
WantedBy=multi-user.target\n",
        xray_work_dir.display(),
        crate::ops::MANAGED_XRAY_GOMEMLIMIT,
        crate::ops::MANAGED_XRAY_GOGC,
        marker,
        pre,
        exec,
        boundary
    )
}

pub(crate) fn render_openrc_xray_script(mode: Option<GuardMode>) -> String {
    let command = if matches!(mode, Some(GuardMode::Enforced)) {
        "command=\"/usr/local/bin/xp-ops\"\ncommand_args=\"_ingress-guard-exec\"\n"
    } else {
        "command=\"/usr/local/bin/xray\"\ncommand_args=\"run -c /etc/xray/config.json\"\n"
    };
    let pre = match mode {
        Some(GuardMode::Enforced) => concat!(
            "\nstart_pre() {\n",
            "  /usr/local/bin/xp-ops _ingress-guard-prepare\n",
            "  result=$?\n",
            "  if [ \"$result\" -ne 0 ]; then\n",
            "    return \"$result\"\n",
            "  fi\n",
            "}\n",
        ),
        Some(GuardMode::Observe) => concat!(
            "\nstart_pre() {\n",
            "  /usr/local/bin/xp-ops _ingress-guard-prepare\n",
            "  result=$?\n",
            "  if [ \"$result\" -ne 0 ]; then\n",
            "    # Observe mode records failures but keeps direct Xray startup.\n",
            "    return 0\n",
            "  fi\n",
            "}\n",
        ),
        None => "",
    };
    format!(
        concat!(
            "#!/sbin/openrc-run\n\n",
            "name=\"xray\"\n",
            "description=\"xray (local proxy runtime)\"\n\n",
            "# Managed by xp-ops ingress-guard service boundary\n",
            "{}command_user=\"xray:xray\"\n",
            "export GOMEMLIMIT=\"${{GOMEMLIMIT:-{}}}\"\n",
            "export GOGC=\"${{GOGC:-{}}}\"\n\n",
            "# Ensure automatic recovery on crashes without busy-looping.\n",
            "supervisor=supervise-daemon\n",
            "respawn_delay=2\n",
            "respawn_max=0\n",
            "{}\n",
            "depend() {{\n  need net\n}}\n",
        ),
        command,
        crate::ops::MANAGED_XRAY_GOMEMLIMIT,
        crate::ops::MANAGED_XRAY_GOGC,
        pre
    )
}

fn legacy_xray_runtime_defaults(rendered: String) -> String {
    rendered
        .replace(
            &format!(
                "Environment=GOMEMLIMIT={}",
                crate::ops::MANAGED_XRAY_GOMEMLIMIT
            ),
            &format!(
                "Environment=GOMEMLIMIT={}",
                crate::ops::LEGACY_XRAY_GOMEMLIMIT
            ),
        )
        .replace(
            &format!("Environment=GOGC={}", crate::ops::MANAGED_XRAY_GOGC),
            &format!("Environment=GOGC={}", crate::ops::LEGACY_XRAY_GOGC),
        )
        .replace(
            &format!("${{GOMEMLIMIT:-{}}}", crate::ops::MANAGED_XRAY_GOMEMLIMIT),
            &format!("${{GOMEMLIMIT:-{}}}", crate::ops::LEGACY_XRAY_GOMEMLIMIT),
        )
        .replace(
            &format!("${{GOGC:-{}}}", crate::ops::MANAGED_XRAY_GOGC),
            &format!("${{GOGC:-{}}}", crate::ops::LEGACY_XRAY_GOGC),
        )
}

pub(crate) fn is_generated_legacy_systemd_xray_unit(raw: &str) -> bool {
    let Some(work_dir) = raw
        .lines()
        .find_map(|line| line.strip_prefix("WorkingDirectory="))
        .map(PathBuf::from)
    else {
        return false;
    };
    let direct = legacy_xray_runtime_defaults(render_systemd_xray_unit(&work_dir, None));
    [
        direct.clone(),
        legacy_xray_runtime_defaults(render_systemd_xray_unit(
            &work_dir,
            Some(GuardMode::Enforced),
        )),
        legacy_xray_runtime_defaults(render_systemd_xray_unit(
            &work_dir,
            Some(GuardMode::Observe),
        )),
        direct.replacen(XRAY_SERVICE_MARKER, "", 1),
    ]
    .iter()
    .any(|expected| raw == expected)
}

pub(crate) fn is_generated_legacy_openrc_xray_script(raw: &str) -> bool {
    let direct = legacy_xray_runtime_defaults(render_openrc_xray_script(None));
    [
        direct.clone(),
        legacy_xray_runtime_defaults(render_openrc_xray_script(Some(GuardMode::Enforced))),
        legacy_xray_runtime_defaults(render_openrc_xray_script(Some(GuardMode::Observe))),
        direct.replacen(XRAY_SERVICE_MARKER, "", 1),
    ]
    .iter()
    .any(|expected| raw == expected)
}

pub(crate) fn migrate_legacy_openrc_xray_script(raw: &str) -> String {
    raw.replace(
        &format!("${{GOMEMLIMIT:-{}}}", crate::ops::LEGACY_XRAY_GOMEMLIMIT),
        &format!("${{GOMEMLIMIT:-{}}}", crate::ops::MANAGED_XRAY_GOMEMLIMIT),
    )
    .replace(
        &format!("${{GOGC:-{}}}", crate::ops::LEGACY_XRAY_GOGC),
        &format!("${{GOGC:-{}}}", crate::ops::MANAGED_XRAY_GOGC),
    )
}
