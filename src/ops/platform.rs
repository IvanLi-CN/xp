use crate::ops::paths::Paths;
use std::fs;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Distro {
    Arch,
    Debian,
    Alpine,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InitSystem {
    Systemd,
    OpenRc,
    None,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CpuArch {
    X86_64,
    Aarch64,
    Other(String),
}

impl CpuArch {
    pub fn normalize(&self) -> Option<&'static str> {
        match self {
            CpuArch::X86_64 => Some("x86_64"),
            CpuArch::Aarch64 => Some("aarch64"),
            CpuArch::Other(_) => None,
        }
    }
}

pub fn detect_cpu_arch() -> CpuArch {
    match std::env::consts::ARCH {
        "x86_64" => CpuArch::X86_64,
        "aarch64" => CpuArch::Aarch64,
        other => CpuArch::Other(other.to_string()),
    }
}

pub fn detect_distro(paths: &Paths) -> Result<Distro, String> {
    if let Ok(v) = std::env::var("XP_OPS_DISTRO") {
        return parse_distro(&v).ok_or_else(|| format!("unknown XP_OPS_DISTRO={v}"));
    }

    let os_release = paths.map_abs(std::path::Path::new("/etc/os-release"));
    let content =
        fs::read_to_string(os_release).map_err(|e| format!("read /etc/os-release: {e}"))?;

    let mut id = None::<String>;
    for line in content.lines() {
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        if k == "ID" {
            id = Some(v.trim_matches('"').to_string());
            break;
        }
    }

    match id.as_deref() {
        Some("arch") => Ok(Distro::Arch),
        Some("debian") | Some("ubuntu") => Ok(Distro::Debian),
        Some("alpine") => Ok(Distro::Alpine),
        Some(other) => Err(format!("unsupported distro: ID={other}")),
        None => Err("unsupported distro: missing ID in /etc/os-release".to_string()),
    }
}

fn parse_distro(v: &str) -> Option<Distro> {
    match v {
        "arch" => Some(Distro::Arch),
        "debian" => Some(Distro::Debian),
        "alpine" => Some(Distro::Alpine),
        _ => None,
    }
}

pub fn detect_init_system(distro: Distro, requested: Option<InitSystem>) -> InitSystem {
    if let Some(s) = requested {
        return s;
    }
    match distro {
        Distro::Alpine => InitSystem::OpenRc,
        Distro::Arch | Distro::Debian => InitSystem::Systemd,
    }
}
