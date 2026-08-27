use super::{ExitError, GuardConfig, GuardMode, OWNERSHIP_MARKER, TABLE_NAME};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

pub(super) fn render(config: &GuardConfig) -> Result<String, ExitError> {
    super::validate_config(config)?;
    let level = config
        .cgroup
        .split('/')
        .filter(|part| !part.is_empty())
        .count();
    if level == 0 {
        return Err(ExitError::new(
            2,
            "cgroup_read_failed: root cgroup is not a listener",
        ));
    }
    let (global_action, source_v4_action, source_v6_action) = if config.mode == GuardMode::Observe {
        (
            "counter name global_over_limit return",
            "counter name source_v4_over_limit return",
            "counter name source_v6_over_limit return",
        )
    } else {
        (
            "counter name global_over_limit drop",
            "counter name source_v4_over_limit drop",
            "counter name source_v6_over_limit drop",
        )
    };
    let global = format!(
        concat!(
            "    socket cgroupv2 level {level} \"{}\" ",
            "iifname != \"lo\" tcp flags & (syn | ack | rst) == syn ",
            "limit rate over {}/second burst {} packets {global_action}\n",
        ),
        config.cgroup,
        config.global_rate,
        config.global_burst,
        level = level,
        global_action = global_action,
    );
    let source_v4 = format!(
        concat!(
            "    socket cgroupv2 level {level} \"{}\" ",
            "iifname != \"lo\" tcp flags & (syn | ack | rst) == syn ",
            "meter source_v4 size {SOURCE_METER_SIZE} ",
            "{{ ip saddr timeout {SOURCE_METER_TIMEOUT} ",
            "limit rate over {}/second burst {} packets ",
            "}} {source_v4_action}\n",
        ),
        config.cgroup,
        config.source_rate,
        config.source_burst,
        level = level,
        SOURCE_METER_SIZE = super::SOURCE_METER_SIZE,
        SOURCE_METER_TIMEOUT = super::SOURCE_METER_TIMEOUT,
        source_v4_action = source_v4_action,
    );
    let source_v6 = format!(
        concat!(
            "    socket cgroupv2 level {level} \"{}\" ",
            "iifname != \"lo\" tcp flags & (syn | ack | rst) == syn ",
            "meter source_v6 size {SOURCE_METER_SIZE} ",
            "{{ ip6 saddr timeout {SOURCE_METER_TIMEOUT} ",
            "limit rate over {}/second burst {} packets ",
            "}} {source_v6_action}\n",
        ),
        config.cgroup,
        config.source_rate,
        config.source_burst,
        level = level,
        SOURCE_METER_SIZE = super::SOURCE_METER_SIZE,
        SOURCE_METER_TIMEOUT = super::SOURCE_METER_TIMEOUT,
        source_v6_action = source_v6_action,
    );
    let admitted = format!(
        concat!(
            "    socket cgroupv2 level {level} \"{}\" ",
            "iifname != \"lo\" tcp flags & (syn | ack | rst) == syn ",
            "counter name admitted_syns return\n",
        ),
        config.cgroup,
        level = level,
    );
    Ok(format!(
        concat!(
            "table inet {} {{\n",
            "  comment \"{}\"\n",
            "  counter global_over_limit {{}}\n",
            "  counter source_v4_over_limit {{}}\n",
            "  counter source_v6_over_limit {{}}\n",
            "  counter admitted_syns {{}}\n",
        ),
        TABLE_NAME, OWNERSHIP_MARKER,
    ) + "  chain input {\n    type filter hook input priority -300; policy accept;\n"
        + &global
        + &source_v4
        + &source_v6
        + &admitted
        + "  }\n}\n")
}

pub(super) fn check(program: &str) -> Result<(), ExitError> {
    run(&["--check", "-f", "-"], program)
}

pub(super) fn apply(program: &str) -> Result<(), ExitError> {
    run(&["-f", "-"], program)
}

fn run(args: &[&str], program: &str) -> Result<(), ExitError> {
    let mut command = Command::new(binary());
    command
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|error| ExitError::new(3, format!("nft_failed: {error}")))?;
    child
        .stdin
        .take()
        .ok_or_else(|| ExitError::new(3, "nft_failed: stdin unavailable"))?
        .write_all(program.as_bytes())
        .map_err(|error| ExitError::new(3, format!("nft_failed: {error}")))?;
    let output = child
        .wait_with_output()
        .map_err(|error| ExitError::new(3, format!("nft_failed: {error}")))?;
    if output.status.success() {
        Ok(())
    } else {
        let detail = String::from_utf8_lossy(&output.stderr);
        Err(ExitError::new(3, format!("nft_failed: {}", detail.trim())))
    }
}

pub(super) fn binary() -> PathBuf {
    std::env::var_os("XP_OPS_NFT_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("nft"))
}
