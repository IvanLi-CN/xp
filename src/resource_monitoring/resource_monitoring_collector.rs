use std::{
    fs,
    path::{Path, PathBuf},
};

use chrono::Utc;

use super::*;

#[derive(Debug, Clone)]
pub(super) struct ProcCounters {
    total: u64,
    idle: u64,
    iowait: u64,
    process: u64,
    read_bytes: u64,
    write_bytes: u64,
    cpu_capacity: Option<f64>,
}

#[derive(Debug, Default)]
pub(super) struct CollectorState {
    pub(super) system: Option<ProcCounters>,
    pub(super) process: Option<ProcCounters>,
}

#[derive(Debug, Clone)]
pub(super) struct LinuxResourceReader {
    data_dir: PathBuf,
    domain: ResourceDomain,
}

impl LinuxResourceReader {
    pub(super) fn new(data_dir: PathBuf) -> Self {
        let domain = if Path::new("/run/.containerenv").exists()
            || Path::new("/.dockerenv").exists()
            || std::env::var_os("XP_RESOURCE_DOMAIN").as_deref() == Some("cgroup".as_ref())
        {
            ResourceDomain::Cgroup
        } else {
            ResourceDomain::Host
        };
        Self { data_dir, domain }
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

        let process = read_process_counters();
        let xp_metrics = read_runtime_metrics(process.as_ref(), state.process.as_ref());
        state.process = process;
        if xp_metrics.cpu_percent.capability != Capability::Supported {
            capability = capability.worst(Capability::Partial);
        }
        let runtimes = ResourceRole::ALL
            .into_iter()
            .map(|role| {
                if role == ResourceRole::Xp {
                    RuntimeSnapshot {
                        role,
                        state: "managed".to_string(),
                        capability: xp_metrics_capability(&xp_metrics),
                        metrics: xp_metrics.clone(),
                    }
                } else {
                    RuntimeSnapshot {
                        role,
                        state: "not_managed".to_string(),
                        capability: Capability::Unsupported,
                        metrics: unsupported_runtime("role_not_managed"),
                    }
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

fn read_runtime_metrics(
    process: Option<&ProcCounters>,
    previous: Option<&ProcCounters>,
) -> RuntimeMetrics {
    let Some(process) = process else {
        return unsupported_runtime("proc_process_unreadable");
    };
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
        process
            .read_bytes
            .checked_sub(previous.read_bytes)
            .map(|value| value as f64 / SAMPLE_INTERVAL.as_secs() as f64)
    });
    RuntimeMetrics {
        cpu_percent,
        rss_bytes: read_proc_status_value("VmRSS")
            .map(Measurement::supported)
            .unwrap_or_else(|| Measurement::unsupported("proc_status_unreadable")),
        pss_bytes: read_pss_bytes()
            .map(Measurement::supported)
            .unwrap_or_else(|| Measurement::unsupported("proc_pss_unreadable")),
        read_bytes_per_second: read_rate
            .map(Measurement::supported)
            .unwrap_or_else(|| Measurement::unsupported("proc_io_unreadable")),
        write_bytes_per_second: previous
            .and_then(|previous| {
                process
                    .write_bytes
                    .checked_sub(previous.write_bytes)
                    .map(|value| value as f64 / SAMPLE_INTERVAL.as_secs() as f64)
            })
            .map(Measurement::supported)
            .unwrap_or_else(|| Measurement::unsupported("proc_io_unreadable")),
        fd_count: fs::read_dir("/proc/self/fd")
            .map(|entries| entries.count() as u64)
            .map(Measurement::supported)
            .unwrap_or_else(|_| Measurement::unsupported("proc_fd_unreadable")),
        thread_count: read_proc_status_value("Threads")
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
        total: values.iter().sum(),
        idle: values[3],
        iowait: values[4],
        process: 0,
        read_bytes: 0,
        write_bytes: 0,
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
        total: usage,
        idle: 0,
        iowait: 0,
        process: 0,
        read_bytes: 0,
        write_bytes: 0,
        cpu_capacity: Some(capacity.max(0.001)),
    })
}

fn read_process_counters() -> Option<ProcCounters> {
    let stat = fs::read_to_string("/proc/self/stat").ok()?;
    let fields = stat
        .rsplit_once(") ")?
        .1
        .split_whitespace()
        .collect::<Vec<_>>();
    let utime = fields.get(11)?.parse::<u64>().ok()?;
    let stime = fields.get(12)?.parse::<u64>().ok()?;
    let io = fs::read_to_string("/proc/self/io").ok()?;
    let mut read_bytes = 0;
    let mut write_bytes = 0;
    for line in io.lines() {
        if let Some(value) = line.strip_prefix("read_bytes:") {
            read_bytes = value.trim().parse().ok()?;
        }
        if let Some(value) = line.strip_prefix("write_bytes:") {
            write_bytes = value.trim().parse().ok()?;
        }
    }
    Some(ProcCounters {
        total: 0,
        idle: 0,
        iowait: 0,
        process: utime + stime,
        read_bytes,
        write_bytes,
        cpu_capacity: None,
    })
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

fn read_proc_status_value(key: &str) -> Option<u64> {
    let contents = fs::read_to_string("/proc/self/status").ok()?;
    let line = contents.lines().find(|line| line.starts_with(key))?;
    let value = line.split_whitespace().nth(1)?.parse::<u64>().ok()?;
    if key == "VmRSS" {
        Some(value.saturating_mul(1024))
    } else {
        Some(value)
    }
}

fn read_pss_bytes() -> Option<u64> {
    let contents = fs::read_to_string("/proc/self/smaps_rollup").ok()?;
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
        total_bytes: stats.f_blocks as u64 * stats.f_frsize,
        available_bytes: stats.f_bavail as u64 * stats.f_frsize,
        total_inodes: stats.f_files as u64,
        available_inodes: stats.f_favail as u64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counter_rates_never_go_negative() {
        let previous = ProcCounters {
            total: 100,
            idle: 50,
            iowait: 5,
            process: 10,
            read_bytes: 100,
            write_bytes: 100,
            cpu_capacity: None,
        };
        let current = ProcCounters {
            total: 90,
            idle: 40,
            iowait: 4,
            process: 8,
            read_bytes: 10,
            write_bytes: 10,
            cpu_capacity: None,
        };
        assert_eq!(percent_from_delta(Some(&previous), &current, false), 0.0);
    }
}
