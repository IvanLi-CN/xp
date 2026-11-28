use std::{
    collections::BTreeMap,
    fs,
    io::{BufRead, BufReader},
    net::{IpAddr, SocketAddr},
    path::Path,
};

use super::{
    ConnectionClass, FullSiteMinuteSample, MAX_ESTABLISHED_TCP_CONNECTIONS,
    TcpConnectionHistoryError,
};
use crate::config::Config;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcTcpRow {
    pub local_ip: IpAddr,
    pub local_port: u16,
    pub remote_ip: IpAddr,
    pub remote_port: u16,
    pub uid: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControlListener {
    pub address: SocketAddr,
    pub subject: &'static str,
}

impl ControlListener {
    pub const fn new(address: SocketAddr, subject: &'static str) -> Self {
        Self { address, subject }
    }
}

pub fn control_listeners(config: &Config) -> Vec<ControlListener> {
    let reverse = SocketAddr::from((std::net::Ipv4Addr::LOCALHOST, 10086));
    vec![
        ControlListener::new(config.bind, "xp_api"),
        ControlListener::new(config.xray_api_addr, "xray_api"),
        ControlListener::new(reverse, "reverse"),
    ]
}

pub fn collect_full_site_minute(
    listen_ports: &BTreeMap<u16, String>,
    control: &[ControlListener],
    runtime_uids: &BTreeMap<u32, String>,
) -> Result<FullSiteMinuteSample, TcpConnectionHistoryError> {
    collect_full_site_minute_from_paths(
        Path::new("/proc/net/tcp"),
        Path::new("/proc/net/tcp6"),
        listen_ports,
        control,
        runtime_uids,
    )
}

fn collect_full_site_minute_from_paths(
    tcp_path: &Path,
    tcp6_path: &Path,
    listen_ports: &BTreeMap<u16, String>,
    control: &[ControlListener],
    runtime_uids: &BTreeMap<u32, String>,
) -> Result<FullSiteMinuteSample, TcpConnectionHistoryError> {
    let mut counts = BTreeMap::<(ConnectionClass, String), u32>::new();
    let mut inspected = 0_usize;
    let mut truncated = false;
    let mut partial = false;
    let mut opened = 0_usize;

    'files: for path in [tcp_path, tcp6_path] {
        let file = match fs::File::open(path) {
            Ok(file) => file,
            Err(_) => {
                partial = true;
                continue;
            }
        };
        opened += 1;
        for (line_index, line) in BufReader::new(file).lines().enumerate() {
            let raw_line = match line {
                Ok(line) => line,
                Err(_) => {
                    partial = true;
                    continue;
                }
            };
            if line_index == 0 {
                continue;
            }
            let line = raw_line.trim();
            if line.is_empty() {
                continue;
            }
            let columns = line.split_whitespace().collect::<Vec<_>>();
            if columns.len() < 8 || columns[3] != "01" {
                if columns.len() < 8 {
                    partial = true;
                }
                continue;
            }
            let Ok((local_ip, local_port)) = parse_proc_addr_port(columns[1]) else {
                partial = true;
                continue;
            };
            let Ok((remote_ip, remote_port)) = parse_proc_addr_port(columns[2]) else {
                partial = true;
                continue;
            };
            if remote_port == 0 {
                continue;
            }
            let uid = match columns[7].parse::<u32>() {
                Ok(uid) => uid,
                Err(_) => {
                    partial = true;
                    0
                }
            };
            if inspected >= MAX_ESTABLISHED_TCP_CONNECTIONS {
                truncated = true;
                break 'files;
            }
            inspected += 1;
            let row = ProcTcpRow {
                local_ip,
                local_port,
                remote_ip,
                remote_port,
                uid,
            };
            let (class, subject) = classify_row(&row, listen_ports, control, runtime_uids);
            counts
                .entry((class, subject))
                .and_modify(|count| *count = count.saturating_add(1))
                .or_insert(1);
        }
    }

    if opened == 0 {
        return Err(TcpConnectionHistoryError::Unsupported(
            "no readable /proc/net/tcp files".to_string(),
        ));
    }

    Ok(FullSiteMinuteSample::new(
        counts,
        truncated,
        partial,
        vec![
            ConnectionClass::Inbound,
            ConnectionClass::ControlPlane,
            ConnectionClass::OutboundRuntime,
            ConnectionClass::OutboundOther,
        ],
    ))
}

fn classify_row(
    row: &ProcTcpRow,
    listen_ports: &BTreeMap<u16, String>,
    control: &[ControlListener],
    runtime_uids: &BTreeMap<u32, String>,
) -> (ConnectionClass, String) {
    if let Some(endpoint_id) = listen_ports.get(&row.local_port) {
        return (ConnectionClass::Inbound, endpoint_id.clone());
    }
    if let Some(subject) = control_subject(row, control) {
        return (ConnectionClass::ControlPlane, subject.to_string());
    }
    if let Some(runtime) = runtime_uids.get(&row.uid) {
        return (ConnectionClass::OutboundRuntime, runtime.clone());
    }
    (ConnectionClass::OutboundOther, row.uid.to_string())
}

fn control_subject(row: &ProcTcpRow, control: &[ControlListener]) -> Option<&'static str> {
    for listener in control {
        let address = listener.address;
        if row.local_port == address.port() && listener_matches_local(address, row.local_ip) {
            return Some(listener.subject);
        }
        if row.remote_port == address.port() && address.ip() == row.remote_ip {
            return Some(listener.subject);
        }
    }
    None
}

fn listener_matches_local(listener: SocketAddr, local: IpAddr) -> bool {
    match listener.ip() {
        IpAddr::V4(ip) if ip.is_unspecified() => local.is_ipv4(),
        IpAddr::V6(ip) if ip.is_unspecified() => local.is_ipv6(),
        ip => ip == local,
    }
}

pub fn resolve_runtime_uids(data_dir: &Path) -> (BTreeMap<u32, String>, bool) {
    let container = Path::new("/run/.containerenv").exists()
        || Path::new("/.dockerenv").exists()
        || std::env::var_os("XP_RESOURCE_DOMAIN").as_deref() == Some("cgroup".as_ref());
    if container {
        resolve_container_runtime_uids(data_dir)
    } else {
        resolve_host_runtime_uids(Path::new("/etc/passwd"))
    }
}

pub fn resolve_host_runtime_uids(passwd_path: &Path) -> (BTreeMap<u32, String>, bool) {
    let raw = fs::read_to_string(passwd_path).unwrap_or_default();
    let mut uids = BTreeMap::new();
    for line in raw.lines() {
        let mut fields = line.split(':');
        let Some(name) = fields.next() else {
            continue;
        };
        let Some(_password) = fields.next() else {
            continue;
        };
        let Some(uid) = fields.next().and_then(|value| value.parse::<u32>().ok()) else {
            continue;
        };
        match name {
            "xray" => {
                uids.insert(uid, "xray".to_string());
            }
            "xp-proxy" => {
                uids.insert(uid, "xp-proxy".to_string());
            }
            _ => {}
        }
    }
    let unresolved = uids.is_empty();
    (uids, unresolved)
}

fn resolve_container_runtime_uids(data_dir: &Path) -> (BTreeMap<u32, String>, bool) {
    let raw =
        fs::read(data_dir.join(crate::resource_monitoring::CONTAINER_RUNTIME_IDENTITIES_FILE)).ok();
    let pid = raw
        .as_deref()
        .and_then(|raw| serde_json::from_slice::<serde_json::Value>(raw).ok())
        .and_then(|value| {
            value
                .get("xray")
                .and_then(|xray| xray.get("pid"))
                .and_then(serde_json::Value::as_u64)
        })
        .and_then(|pid| u32::try_from(pid).ok());
    let uid = pid.and_then(read_proc_uid);
    match uid {
        Some(uid) => (BTreeMap::from([(uid, "xray".to_string())]), false),
        None => (BTreeMap::new(), true),
    }
}

fn read_proc_uid(pid: u32) -> Option<u32> {
    let raw = fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    raw.lines()
        .find_map(|line| line.strip_prefix("Uid:"))
        .and_then(|value| value.split_whitespace().next())
        .and_then(|value| value.parse().ok())
}

fn parse_proc_addr_port(value: &str) -> Result<(IpAddr, u16), ()> {
    let (addr_hex, port_hex) = value.split_once(':').ok_or(())?;
    let port = u16::from_str_radix(port_hex, 16).map_err(|_| ())?;
    let ip = match addr_hex.len() {
        8 => parse_ipv4_hex(addr_hex)?,
        32 => parse_ipv6_hex(addr_hex)?,
        _ => return Err(()),
    };
    Ok((ip, port))
}

fn parse_ipv4_hex(value: &str) -> Result<IpAddr, ()> {
    let raw = u32::from_str_radix(value, 16).map_err(|_| ())?;
    Ok(IpAddr::from(raw.to_le_bytes()))
}

fn parse_ipv6_hex(value: &str) -> Result<IpAddr, ()> {
    let mut out = [0_u8; 16];
    for index in 0..4 {
        let start = index * 8;
        let chunk = &value[start..start + 8];
        let parsed = u32::from_str_radix(chunk, 16).map_err(|_| ())?;
        out[index * 4..index * 4 + 4].copy_from_slice(&parsed.to_le_bytes());
    }
    Ok(IpAddr::from(out))
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    fn endpoint(port: u16, id: &str) -> BTreeMap<u16, String> {
        BTreeMap::from([(port, id.to_string())])
    }

    fn control() -> Vec<ControlListener> {
        vec![
            ControlListener::new("0.0.0.0:62416".parse().unwrap(), "xp_api"),
            ControlListener::new("127.0.0.1:10085".parse().unwrap(), "xray_api"),
            ControlListener::new("127.0.0.1:10086".parse().unwrap(), "reverse"),
        ]
    }

    fn proc_file(rows: &[&str]) -> tempfile::NamedTempFile {
        let temp = tempfile::NamedTempFile::new().unwrap();
        let mut file = temp.reopen().unwrap();
        writeln!(file, "sl local_address rem_address st uid").unwrap();
        for row in rows {
            writeln!(file, "{row}").unwrap();
        }
        temp
    }

    #[test]
    fn classifies_inbound_control_runtime_and_other() {
        let tcp = proc_file(&[
            "0: 0100007F:01BB 0200007F:9C40 01 00000000:00000000 00000000:00000000 0 1000 0 1",
            "1: 00000000:F3D0 0100007F:9C41 01 00000000:00000000 00000000:00000000 0 1001 0 2",
            "2: 0100007F:9C42 0100007F:2765 01 00000000:00000000 00000000:00000000 0 1002 0 3",
            "3: 0100007F:9C43 0100007F:2766 01 00000000:00000000 00000000:00000000 0 1003 0 4",
            "4: 0100007F:9C44 0100007F:9C45 01 00000000:00000000 00000000:00000000 0 1004 0 5",
            "5: 0100007F:9C46 0100007F:9C47 01 00000000:00000000 00000000:00000000 0 1002 0 6",
        ]);
        let missing = Path::new("/nonexistent-xp-tcp6");
        let sample = collect_full_site_minute_from_paths(
            tcp.path(),
            missing,
            &endpoint(443, "ep-a"),
            &control(),
            &BTreeMap::from([(1002, "xray".to_string())]),
        )
        .unwrap();
        assert_eq!(sample.total, 6);
        assert_eq!(sample.class_total(ConnectionClass::Inbound), 1);
        assert_eq!(sample.class_total(ConnectionClass::ControlPlane), 3);
        assert_eq!(sample.class_total(ConnectionClass::OutboundRuntime), 1);
        assert_eq!(sample.class_total(ConnectionClass::OutboundOther), 1);
        assert!(sample.partial);
    }

    #[test]
    fn truncates_at_the_row_bound() {
        let rows = (0..=MAX_ESTABLISHED_TCP_CONNECTIONS)
            .map(|index| {
                format!(
                    "{index}: 0100007F:01BB 0200007F:9C40 01 \
                     00000000:00000000 00000000:00000000 0 1000 0 1"
                )
            })
            .collect::<Vec<_>>();
        let row_refs = rows.iter().map(String::as_str).collect::<Vec<_>>();
        let tcp = proc_file(&row_refs);
        let missing = Path::new("/nonexistent-xp-tcp6");
        let sample = collect_full_site_minute_from_paths(
            tcp.path(),
            missing,
            &BTreeMap::new(),
            &control(),
            &BTreeMap::new(),
        )
        .unwrap();
        assert!(sample.truncated);
        assert_eq!(sample.total as usize, MAX_ESTABLISHED_TCP_CONNECTIONS);
    }

    #[test]
    fn resolves_fixed_runtime_users() {
        let passwd = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(
            passwd.path(),
            concat!(
                "xray:x:998:998::/var/lib/xray:/usr/sbin/nologin\n",
                "xp-proxy:x:999:999::/var/lib/xp-proxy:/usr/sbin/nologin\n"
            ),
        )
        .unwrap();
        let (uids, unresolved) = resolve_host_runtime_uids(passwd.path());
        assert!(!unresolved);
        assert_eq!(uids.get(&998).map(String::as_str), Some("xray"));
        assert_eq!(uids.get(&999).map(String::as_str), Some("xp-proxy"));
    }
}
