use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use super::*;
use chrono::Utc;

#[derive(Debug, Clone)]
pub(super) struct ProcCounters {
    pid: u32,
    total: u64,
    idle: u64,
    iowait: u64,
    process: u64,
    read_bytes: Option<u64>,
    write_bytes: Option<u64>,
    cpu_capacity: Option<f64>,
}

#[derive(Debug, Default)]
pub(super) struct CollectorState {
    pub(super) system: Option<ProcCounters>,
    pub(super) runtimes: BTreeMap<ResourceRole, ProcCounters>,
}

#[derive(Debug, Clone)]
pub(super) struct LinuxResourceReader {
    data_dir: PathBuf,
    domain: ResourceDomain,
    runtime_targets: ManagedRuntimeTargets,
}

impl LinuxResourceReader {
    pub(super) fn with_runtime_targets(
        data_dir: PathBuf,
        runtime_targets: ManagedRuntimeTargets,
    ) -> Self {
        let domain = if Path::new("/run/.containerenv").exists()
            || Path::new("/.dockerenv").exists()
            || std::env::var_os("XP_RESOURCE_DOMAIN").as_deref() == Some("cgroup".as_ref())
        {
            ResourceDomain::Cgroup
        } else {
            ResourceDomain::Host
        };
        Self {
            data_dir,
            domain,
            runtime_targets,
        }
    }

    pub(super) fn read(&self, node_id: &str, state: &mut CollectorState) -> ResourceSnapshot {
        let observed_at = Utc::now();
        let mut capability = Capability::Supported;
        let system_counters = match self.domain {
            ResourceDomain::Host => read_proc_stat(),
            ResourceDomain::Cgroup => read_cgroup_cpu(),
        };
        let system = system_counters
            .as_ref()
            .map(|counters| percent_from_delta(state.system.as_ref(), counters, false));
        let iowait = (self.domain == ResourceDomain::Host)
            .then(|| {
                system_counters
                    .as_ref()
                    .map(|counters| percent_from_delta(state.system.as_ref(), counters, true))
            })
            .flatten();
        state.system = system_counters;
        let domain = DomainMetrics {
            cpu_busy_percent: system
                .map(Measurement::supported)
                .unwrap_or_else(|| Measurement::unsupported("proc_stat_unreadable")),
            cpu_iowait_percent: iowait
                .map(Measurement::supported)
                .unwrap_or_else(|| Measurement::unsupported("proc_stat_unreadable")),
            load1: (self.domain == ResourceDomain::Host)
                .then(read_loadavg)
                .flatten()
                .map(Measurement::supported)
                .unwrap_or_else(|| Measurement::unsupported("loadavg_unreadable")),
            memory_total_bytes: read_memory_total(self.domain)
                .map(Measurement::supported)
                .unwrap_or_else(|| Measurement::unsupported("memory_total_unreadable")),
            memory_available_bytes: read_memory_available(self.domain)
                .map(Measurement::supported)
                .unwrap_or_else(|| Measurement::unsupported("memory_available_unreadable")),
            swap_total_bytes: read_swap_total(self.domain)
                .map(Measurement::supported)
                .unwrap_or_else(|| Measurement::unsupported("swap_total_unreadable")),
            swap_free_bytes: read_swap_free(self.domain)
                .map(Measurement::supported)
                .unwrap_or_else(|| Measurement::unsupported("swap_free_unreadable")),
            filesystems: read_filesystems(&self.data_dir),
        };
        capability = capability.worst(domain.cpu_busy_percent.capability);
        capability = capability.worst(domain.memory_available_bytes.capability);
        if domain
            .filesystems
            .iter()
            .any(|filesystem| filesystem.capability != Capability::Supported)
        {
            capability = capability.worst(Capability::Partial);
        }

        let runtimes = ResourceRole::ALL
            .into_iter()
            .map(|role| {
                let target = self.runtime_target(role);
                let counters = target.pid().and_then(read_process_counters_for_pid);
                let metrics = counters
                    .as_ref()
                    .map(|current| read_runtime_metrics(current, state.runtimes.get(&role)))
                    .unwrap_or_else(|| unsupported_runtime(target.reason_code()));
                if let Some(counters) = counters {
                    state.runtimes.insert(role, counters);
                } else {
                    state.runtimes.remove(&role);
                }
                RuntimeSnapshot {
                    role,
                    state: target.state().to_string(),
                    capability: xp_metrics_capability(&metrics),
                    metrics,
                }
            })
            .collect::<Vec<_>>();
        let runtime_capability = runtimes.iter().fold(Capability::Supported, |acc, item| {
            acc.worst(item.capability)
        });
        capability = capability.worst(match runtime_capability {
            Capability::Unsupported => Capability::Partial,
            capability => capability,
        });
        if runtimes
            .iter()
            .any(|runtime| runtime.state == "not_managed")
        {
            capability = capability.worst(Capability::Partial);
        }
        ResourceSnapshot {
            node_id: node_id.to_string(),
            observed_at: observed_at.to_rfc3339(),
            resource_domain: self.domain,
            capture_state: "active".to_string(),
            capability,
            domain,
            runtimes,
        }
    }

    fn runtime_target(&self, role: ResourceRole) -> RuntimeTarget {
        if role == ResourceRole::Xp {
            return RuntimeTarget::Managed(std::process::id());
        }
        if role == ResourceRole::Canary {
            return if canary_is_managed(&self.data_dir) {
                RuntimeTarget::ManagedUnavailable("runtime_not_separable")
            } else {
                RuntimeTarget::NotManaged
            };
        }
        if self.domain == ResourceDomain::Cgroup {
            return container_runtime_target(&self.data_dir, role);
        }
        let (unit, service) = match role {
            ResourceRole::Xray => (
                self.runtime_targets.xray_systemd_unit.as_str(),
                self.runtime_targets.xray_openrc_service.as_str(),
            ),
            ResourceRole::Cloudflared => (
                self.runtime_targets.cloudflared_systemd_unit.as_str(),
                self.runtime_targets.cloudflared_openrc_service.as_str(),
            ),
            _ => return RuntimeTarget::NotManaged,
        };
        prefer_runtime_target(
            fixed_systemd_runtime_target(unit),
            fixed_openrc_runtime_target(service),
        )
    }
}

#[derive(Clone, Copy)]
enum RuntimeTarget {
    Managed(u32),
    ManagedUnavailable(&'static str),
    NotManaged,
}

impl RuntimeTarget {
    fn pid(&self) -> Option<u32> {
        match self {
            Self::Managed(pid) => Some(*pid),
            Self::ManagedUnavailable(_) | Self::NotManaged => None,
        }
    }

    fn state(&self) -> &'static str {
        match self {
            Self::Managed(_) | Self::ManagedUnavailable(_) => "managed",
            Self::NotManaged => "not_managed",
        }
    }

    fn reason_code(&self) -> &'static str {
        match self {
            Self::Managed(_) => "proc_process_unreadable",
            Self::ManagedUnavailable(reason) => reason,
            Self::NotManaged => "runtime_not_managed",
        }
    }
}

fn xp_metrics_capability(metrics: &RuntimeMetrics) -> Capability {
    [
        metrics.cpu_percent.capability,
        metrics.rss_bytes.capability,
        metrics.pss_bytes.capability,
        metrics.read_bytes_per_second.capability,
        metrics.write_bytes_per_second.capability,
        metrics.fd_count.capability,
        metrics.thread_count.capability,
    ]
    .into_iter()
    .fold(Capability::Supported, Capability::worst)
}

pub(super) fn unsupported_runtime(reason: &str) -> RuntimeMetrics {
    RuntimeMetrics {
        cpu_percent: Measurement::unsupported(reason),
        rss_bytes: Measurement::unsupported(reason),
        pss_bytes: Measurement::unsupported(reason),
        read_bytes_per_second: Measurement::unsupported(reason),
        write_bytes_per_second: Measurement::unsupported(reason),
        fd_count: Measurement::unsupported(reason),
        thread_count: Measurement::unsupported(reason),
    }
}

pub(crate) fn unsupported_snapshot(node_id: &str) -> ResourceSnapshot {
    ResourceSnapshot {
        node_id: node_id.to_string(),
        observed_at: Utc::now().to_rfc3339(),
        resource_domain: ResourceDomain::Host,
        capture_state: "active".to_string(),
        capability: Capability::Unsupported,
        domain: DomainMetrics {
            cpu_busy_percent: Measurement::unsupported("not_sampled"),
            cpu_iowait_percent: Measurement::unsupported("not_sampled"),
            load1: Measurement::unsupported("not_sampled"),
            memory_total_bytes: Measurement::unsupported("not_sampled"),
            memory_available_bytes: Measurement::unsupported("not_sampled"),
            swap_total_bytes: Measurement::unsupported("not_sampled"),
            swap_free_bytes: Measurement::unsupported("not_sampled"),
            filesystems: Vec::new(),
        },
        runtimes: ResourceRole::ALL
            .into_iter()
            .map(|role| RuntimeSnapshot {
                role,
                state: "not_managed".to_string(),
                capability: Capability::Unsupported,
                metrics: unsupported_runtime("not_sampled"),
            })
            .collect(),
    }
}

fn read_runtime_metrics(process: &ProcCounters, previous: Option<&ProcCounters>) -> RuntimeMetrics {
    let previous = previous.filter(|previous| previous.pid == process.pid);
    let cpu_percent = previous
        .and_then(|previous| {
            let delta = process.process.checked_sub(previous.process)? as f64;
            let ticks_per_second = unsafe { libc::sysconf(libc::_SC_CLK_TCK) }
                .try_into()
                .ok()
                .filter(|value: &u64| *value > 0)
                .unwrap_or(100);
            let denominator = ticks_per_second as f64 * SAMPLE_INTERVAL.as_secs_f64();
            Some(Measurement::supported(
                (delta * 100.0 / denominator).clamp(0.0, 100.0),
            ))
        })
        .unwrap_or_else(|| Measurement::unsupported("counter_baseline"));
    let read_rate = previous.and_then(|previous| {
        previous
            .read_bytes
            .zip(process.read_bytes)
            .and_then(|(previous, current)| current.checked_sub(previous))
            .map(|value| value as f64 / SAMPLE_INTERVAL.as_secs() as f64)
    });
    RuntimeMetrics {
        cpu_percent,
        rss_bytes: read_proc_status_value_for_pid(process.pid, "VmRSS")
            .map(Measurement::supported)
            .unwrap_or_else(|| Measurement::unsupported("proc_status_unreadable")),
        pss_bytes: read_pss_bytes_for_pid(process.pid)
            .map(Measurement::supported)
            .unwrap_or_else(|| Measurement::unsupported("proc_pss_unreadable")),
        read_bytes_per_second: read_rate
            .map(Measurement::supported)
            .unwrap_or_else(|| Measurement::unsupported("proc_io_unreadable")),
        write_bytes_per_second: previous
            .and_then(|previous| {
                previous
                    .write_bytes
                    .zip(process.write_bytes)
                    .and_then(|(previous, current)| current.checked_sub(previous))
                    .map(|value| value as f64 / SAMPLE_INTERVAL.as_secs() as f64)
            })
            .map(Measurement::supported)
            .unwrap_or_else(|| Measurement::unsupported("proc_io_unreadable")),
        fd_count: fs::read_dir(proc_path(process.pid, "fd"))
            .map(|entries| entries.count() as u64)
            .map(Measurement::supported)
            .unwrap_or_else(|_| Measurement::unsupported("proc_fd_unreadable")),
        thread_count: read_proc_status_value_for_pid(process.pid, "Threads")
            .map(Measurement::supported)
            .unwrap_or_else(|| Measurement::unsupported("proc_status_unreadable")),
    }
}

fn read_proc_stat() -> Option<ProcCounters> {
    let contents = fs::read_to_string("/proc/stat").ok()?;
    let line = contents.lines().find(|line| line.starts_with("cpu "))?;
    let values = line
        .split_whitespace()
        .skip(1)
        .map(str::parse::<u64>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    if values.len() < 5 {
        return None;
    }
    Some(ProcCounters {
        pid: 0,
        total: values.iter().sum(),
        idle: values[3],
        iowait: values[4],
        process: 0,
        read_bytes: Some(0),
        write_bytes: Some(0),
        cpu_capacity: None,
    })
}

fn read_cgroup_cpu() -> Option<ProcCounters> {
    let usage = read_cgroup_stat_value("cpu.stat", "usage_usec")?;
    let capacity = fs::read_to_string("/sys/fs/cgroup/cpu.max")
        .ok()
        .and_then(|value| {
            let mut fields = value.split_whitespace();
            let quota = fields.next()?;
            let period = fields.next()?.parse::<f64>().ok()?;
            if quota == "max" {
                return Some(
                    std::thread::available_parallelism()
                        .ok()
                        .map_or(1.0, |value| value.get() as f64),
                );
            }
            Some(quota.parse::<f64>().ok()? / period)
        })?;
    Some(ProcCounters {
        pid: 0,
        total: usage,
        idle: 0,
        iowait: 0,
        process: 0,
        read_bytes: Some(0),
        write_bytes: Some(0),
        cpu_capacity: Some(capacity.max(0.001)),
    })
}

fn read_process_counters_for_pid(pid: u32) -> Option<ProcCounters> {
    let stat = fs::read_to_string(proc_path(pid, "stat")).ok()?;
    let fields = stat
        .rsplit_once(") ")?
        .1
        .split_whitespace()
        .collect::<Vec<_>>();
    let utime = fields.get(11)?.parse::<u64>().ok()?;
    let stime = fields.get(12)?.parse::<u64>().ok()?;
    let (read_bytes, write_bytes) = read_process_io_counters(pid);
    Some(ProcCounters {
        pid,
        total: 0,
        idle: 0,
        iowait: 0,
        process: utime + stime,
        read_bytes,
        write_bytes,
        cpu_capacity: None,
    })
}

fn read_process_io_counters(pid: u32) -> (Option<u64>, Option<u64>) {
    let Ok(io) = fs::read_to_string(proc_path(pid, "io")) else {
        return (None, None);
    };
    let mut read_bytes = None;
    let mut write_bytes = None;
    for line in io.lines() {
        if let Some(value) = line.strip_prefix("read_bytes:") {
            read_bytes = value.trim().parse().ok();
        }
        if let Some(value) = line.strip_prefix("write_bytes:") {
            write_bytes = value.trim().parse().ok();
        }
    }
    (read_bytes, write_bytes)
}

fn container_runtime_target(data_dir: &Path, role: ResourceRole) -> RuntimeTarget {
    let Ok(raw) = fs::read(data_dir.join(CONTAINER_RUNTIME_IDENTITIES_FILE)) else {
        return RuntimeTarget::ManagedUnavailable("runtime_identity_unavailable");
    };
    let Ok(identities) = serde_json::from_slice::<ContainerRuntimeIdentities>(&raw) else {
        return RuntimeTarget::ManagedUnavailable("runtime_identity_invalid");
    };
    let identity = match role {
        ResourceRole::Xray => Some(identities.xray),
        ResourceRole::Cloudflared => identities.cloudflared,
        _ => return RuntimeTarget::NotManaged,
    };
    let Some(identity) = identity else {
        return RuntimeTarget::NotManaged;
    };
    if process_start_time_ticks(identity.pid) == Some(identity.start_time_ticks) {
        RuntimeTarget::Managed(identity.pid)
    } else {
        RuntimeTarget::ManagedUnavailable("runtime_identity_mismatch")
    }
}

#[derive(serde::Deserialize)]
struct ContainerRuntimeIdentities {
    xray: RuntimeIdentity,
    cloudflared: Option<RuntimeIdentity>,
}

#[derive(serde::Deserialize)]
struct RuntimeIdentity {
    pid: u32,
    start_time_ticks: u64,
}

fn fixed_systemd_runtime_target(unit: &str) -> RuntimeTarget {
    if !valid_systemd_unit(unit) {
        return RuntimeTarget::ManagedUnavailable("runtime_identity_invalid");
    }
    let path = Path::new("/sys/fs/cgroup/system.slice")
        .join(unit)
        .join("cgroup.procs");
    if !path.exists() {
        return RuntimeTarget::NotManaged;
    }
    read_single_pid_file(&path)
        .map(RuntimeTarget::Managed)
        .unwrap_or(RuntimeTarget::ManagedUnavailable("runtime_pid_unreadable"))
}

fn fixed_openrc_runtime_target(service: &str) -> RuntimeTarget {
    if !valid_openrc_service(service) {
        return RuntimeTarget::ManagedUnavailable("runtime_identity_invalid");
    }
    let pidfiles = [
        PathBuf::from(format!("/run/supervise-{service}.pid")),
        PathBuf::from(format!("/var/run/supervise-{service}.pid")),
    ];
    let existing_pidfiles = pidfiles
        .iter()
        .filter(|path| path.exists())
        .collect::<Vec<_>>();
    if existing_pidfiles.is_empty() {
        return RuntimeTarget::NotManaged;
    }
    let Some(supervisor) = existing_pidfiles
        .into_iter()
        .find_map(|path| read_single_pid_file(path))
    else {
        return RuntimeTarget::ManagedUnavailable("runtime_pid_unreadable");
    };
    read_single_pid_file(&proc_path(
        supervisor,
        &format!("task/{supervisor}/children"),
    ))
    .map(RuntimeTarget::Managed)
    .unwrap_or(RuntimeTarget::ManagedUnavailable("runtime_pid_unreadable"))
}

fn prefer_runtime_target(primary: RuntimeTarget, fallback: RuntimeTarget) -> RuntimeTarget {
    match (primary, fallback) {
        (managed @ RuntimeTarget::Managed(_), _) | (_, managed @ RuntimeTarget::Managed(_)) => {
            managed
        }
        (unavailable @ RuntimeTarget::ManagedUnavailable(_), _) => unavailable,
        (_, unavailable @ RuntimeTarget::ManagedUnavailable(_)) => unavailable,
        _ => RuntimeTarget::NotManaged,
    }
}

fn valid_systemd_unit(unit: &str) -> bool {
    unit.ends_with(".service")
        && !unit.is_empty()
        && unit
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'@'))
}

fn valid_openrc_service(service: &str) -> bool {
    !service.is_empty()
        && service
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn read_single_pid_file(path: &Path) -> Option<u32> {
    let contents = fs::read_to_string(path).ok()?;
    let mut pids = contents
        .split_whitespace()
        .filter_map(|raw| raw.parse::<u32>().ok());
    let pid = pids.next()?;
    pids.next().is_none().then_some(pid)
}

fn proc_path(pid: u32, entry: &str) -> PathBuf {
    Path::new("/proc").join(pid.to_string()).join(entry)
}

fn process_start_time_ticks(pid: u32) -> Option<u64> {
    process_start_time_ticks_from_stat(&fs::read_to_string(proc_path(pid, "stat")).ok()?)
}

fn process_start_time_ticks_from_stat(stat: &str) -> Option<u64> {
    stat.rsplit_once(") ")?
        .1
        .split_whitespace()
        .nth(19)?
        .parse()
        .ok()
}

fn canary_is_managed(data_dir: &Path) -> bool {
    let path = crate::vless_https_canary::VlessHttpsCanaryPaths::new(data_dir).status_json;
    fs::read(path)
        .ok()
        .and_then(|raw| serde_json::from_slice::<CanaryStatus>(&raw).ok())
        .is_some_and(|status| status.enabled)
}

#[derive(serde::Deserialize)]
struct CanaryStatus {
    enabled: bool,
}

fn percent_from_delta(
    previous: Option<&ProcCounters>,
    current: &ProcCounters,
    iowait: bool,
) -> f64 {
    let Some(previous) = previous else { return 0.0 };
    let total = current.total.saturating_sub(previous.total);
    if total == 0 {
        return 0.0;
    }
    if let Some(capacity) = current.cpu_capacity {
        if iowait {
            return 0.0;
        }
        return (total as f64 * 100.0 / (15_000_000.0 * capacity)).clamp(0.0, 100.0);
    }
    let numerator = if iowait {
        current.iowait.saturating_sub(previous.iowait)
    } else {
        total
            .saturating_sub(current.idle.saturating_sub(previous.idle))
            .saturating_sub(current.iowait.saturating_sub(previous.iowait))
    };
    (numerator as f64 * 100.0 / total as f64).clamp(0.0, 100.0)
}

fn read_loadavg() -> Option<f64> {
    fs::read_to_string("/proc/loadavg")
        .ok()?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

fn meminfo_value(key: &str) -> Option<u64> {
    let contents = fs::read_to_string("/proc/meminfo").ok()?;
    let line = contents.lines().find(|line| line.starts_with(key))?;
    let value = line.split_whitespace().nth(1)?.parse::<u64>().ok()?;
    Some(value.saturating_mul(1024))
}

fn read_cgroup_scalar(file: &str) -> Option<u64> {
    fs::read_to_string(Path::new("/sys/fs/cgroup").join(file))
        .ok()?
        .trim()
        .parse()
        .ok()
}

fn read_cgroup_stat_value(file: &str, key: &str) -> Option<u64> {
    fs::read_to_string(Path::new("/sys/fs/cgroup").join(file))
        .ok()?
        .lines()
        .find_map(|line| {
            let mut fields = line.split_whitespace();
            (fields.next()? == key).then(|| fields.next()?.parse().ok())?
        })
}

fn read_cgroup_limit(file: &str) -> Option<u64> {
    let value = fs::read_to_string(Path::new("/sys/fs/cgroup").join(file)).ok()?;
    (value.trim() != "max").then(|| value.trim().parse().ok())?
}

fn read_memory_total(domain: ResourceDomain) -> Option<u64> {
    match domain {
        ResourceDomain::Host => meminfo_value("MemTotal"),
        ResourceDomain::Cgroup => read_cgroup_limit("memory.max"),
    }
}

fn read_memory_available(domain: ResourceDomain) -> Option<u64> {
    match domain {
        ResourceDomain::Host => meminfo_value("MemAvailable"),
        ResourceDomain::Cgroup => read_cgroup_limit("memory.max").and_then(|limit| {
            read_cgroup_scalar("memory.current").and_then(|used| limit.checked_sub(used))
        }),
    }
}

fn read_swap_total(domain: ResourceDomain) -> Option<u64> {
    match domain {
        ResourceDomain::Host => meminfo_value("SwapTotal"),
        ResourceDomain::Cgroup => read_cgroup_limit("memory.swap.max"),
    }
}

fn read_swap_free(domain: ResourceDomain) -> Option<u64> {
    match domain {
        ResourceDomain::Host => meminfo_value("SwapFree"),
        ResourceDomain::Cgroup => read_cgroup_limit("memory.swap.max").and_then(|limit| {
            read_cgroup_scalar("memory.swap.current").and_then(|used| limit.checked_sub(used))
        }),
    }
}

fn read_proc_status_value_for_pid(pid: u32, key: &str) -> Option<u64> {
    let contents = fs::read_to_string(proc_path(pid, "status")).ok()?;
    let line = contents.lines().find(|line| line.starts_with(key))?;
    let value = line.split_whitespace().nth(1)?.parse::<u64>().ok()?;
    if key == "VmRSS" {
        Some(value.saturating_mul(1024))
    } else {
        Some(value)
    }
}

fn read_pss_bytes_for_pid(pid: u32) -> Option<u64> {
    let contents = fs::read_to_string(proc_path(pid, "smaps_rollup")).ok()?;
    let line = contents.lines().find(|line| line.starts_with("Pss:"))?;
    let value = line.split_whitespace().nth(1)?.parse::<u64>().ok()?;
    Some(value.saturating_mul(1024))
}

fn read_filesystems(data_dir: &Path) -> Vec<FilesystemMetrics> {
    let mut paths = vec![PathBuf::from("/")];
    if data_dir != Path::new("/") {
        paths.push(data_dir.to_path_buf());
    }
    let mut seen = Vec::new();
    paths
        .into_iter()
        .filter_map(|path| {
            let stats = statvfs(&path).ok()?;
            let identity = stats.fsid;
            if seen.contains(&identity) {
                return None;
            }
            seen.push(identity);
            Some(FilesystemMetrics {
                mount: if path == Path::new("/") {
                    "root".to_string()
                } else {
                    "data".to_string()
                },
                capability: Capability::Supported,
                total_bytes: Some(stats.total_bytes),
                available_bytes: Some(stats.available_bytes),
                used_percent: (stats.total_bytes > 0).then(|| {
                    ((stats.total_bytes - stats.available_bytes) as f64 * 100.0)
                        / stats.total_bytes as f64
                }),
                total_inodes: Some(stats.total_inodes),
                available_inodes: Some(stats.available_inodes),
                used_inode_percent: (stats.total_inodes > 0).then(|| {
                    ((stats.total_inodes.saturating_sub(stats.available_inodes)) as f64 * 100.0)
                        / stats.total_inodes as f64
                }),
                reason_code: None,
            })
        })
        .collect()
}

struct VfsStats {
    fsid: u64,
    total_bytes: u64,
    available_bytes: u64,
    total_inodes: u64,
    available_inodes: u64,
}

fn statvfs(path: &Path) -> std::io::Result<VfsStats> {
    let path = std::ffi::CString::new(path.as_os_str().as_encoded_bytes())
        .map_err(|_| std::io::ErrorKind::InvalidInput)?;
    let mut stats = std::mem::MaybeUninit::<libc::statvfs>::zeroed();
    let result = unsafe { libc::statvfs(path.as_ptr(), stats.as_mut_ptr()) };
    if result != 0 {
        return Err(std::io::Error::last_os_error());
    }
    let stats = unsafe { stats.assume_init() };
    Ok(VfsStats {
        fsid: stats.f_fsid,
        total_bytes: Into::<u64>::into(stats.f_blocks) * stats.f_frsize,
        available_bytes: Into::<u64>::into(stats.f_bavail) * stats.f_frsize,
        total_inodes: Into::<u64>::into(stats.f_files),
        available_inodes: Into::<u64>::into(stats.f_favail),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counter_rates_never_go_negative() {
        let previous = ProcCounters {
            pid: 0,
            total: 100,
            idle: 50,
            iowait: 5,
            process: 10,
            read_bytes: Some(100),
            write_bytes: Some(100),
            cpu_capacity: None,
        };
        let current = ProcCounters {
            pid: 0,
            total: 90,
            idle: 40,
            iowait: 4,
            process: 8,
            read_bytes: Some(10),
            write_bytes: Some(10),
            cpu_capacity: None,
        };
        assert_eq!(percent_from_delta(Some(&previous), &current, false), 0.0);
    }

    #[test]
    fn fixed_role_identity_paths_reject_unbounded_names() {
        assert!(valid_systemd_unit("xray.service"));
        assert!(!valid_systemd_unit("xray.service/../../other"));
        assert!(valid_openrc_service("cloudflared"));
        assert!(!valid_openrc_service("cloudflared;ps"));
    }

    #[test]
    fn role_identity_keeps_managed_failures_distinct_from_absence() {
        assert_eq!(RuntimeTarget::NotManaged.state(), "not_managed");
        assert_eq!(
            RuntimeTarget::ManagedUnavailable("runtime_pid_unreadable").state(),
            "managed"
        );
        assert_eq!(
            prefer_runtime_target(
                RuntimeTarget::ManagedUnavailable("runtime_pid_unreadable"),
                RuntimeTarget::Managed(42),
            )
            .pid(),
            Some(42)
        );
    }

    #[test]
    fn replacing_a_runtime_pid_resets_the_rate_baseline() {
        let previous = ProcCounters {
            pid: 10,
            total: 0,
            idle: 0,
            iowait: 0,
            process: 100,
            read_bytes: Some(1_000),
            write_bytes: Some(1_000),
            cpu_capacity: None,
        };
        let current = ProcCounters {
            pid: 11,
            total: 0,
            idle: 0,
            iowait: 0,
            process: 10_000,
            read_bytes: Some(1_000_000),
            write_bytes: Some(1_000_000),
            cpu_capacity: None,
        };

        let metrics = read_runtime_metrics(&current, Some(&previous));
        assert_eq!(
            metrics.cpu_percent.reason_code.as_deref(),
            Some("counter_baseline")
        );
        assert_eq!(
            metrics.read_bytes_per_second.reason_code.as_deref(),
            Some("proc_io_unreadable")
        );
        assert_eq!(
            metrics.write_bytes_per_second.reason_code.as_deref(),
            Some("proc_io_unreadable")
        );
    }

    #[test]
    fn parses_process_start_time_after_parenthesized_command() {
        let stat = "42 (xray worker) S 1 1 1 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 12345 0";
        assert_eq!(process_start_time_ticks_from_stat(stat), Some(12_345));
    }
}
