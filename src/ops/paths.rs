use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Paths {
    root: PathBuf,
}

impl Paths {
    pub fn new(root: PathBuf) -> Self {
        let root = if root.as_os_str().is_empty() {
            PathBuf::from("/")
        } else {
            root
        };
        Self { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn map_abs(&self, abs: &Path) -> PathBuf {
        if abs == Path::new("/") {
            return self.root.clone();
        }
        let stripped = abs.strip_prefix("/").unwrap_or(abs);
        self.root.join(stripped)
    }

    pub fn etc_xp_dir(&self) -> PathBuf {
        self.map_abs(Path::new("/etc/xp"))
    }

    pub fn etc_xp_env(&self) -> PathBuf {
        self.map_abs(Path::new("/etc/xp/xp.env"))
    }

    pub fn etc_xray_dir(&self) -> PathBuf {
        self.map_abs(Path::new("/etc/xray"))
    }

    pub fn etc_xray_config(&self) -> PathBuf {
        self.map_abs(Path::new("/etc/xray/config.json"))
    }

    pub fn etc_cloudflared_dir(&self) -> PathBuf {
        self.map_abs(Path::new("/etc/cloudflared"))
    }

    pub fn etc_cloudflared_config(&self) -> PathBuf {
        self.map_abs(Path::new("/etc/cloudflared/config.yml"))
    }

    pub fn etc_polkit_rules_dir(&self) -> PathBuf {
        self.map_abs(Path::new("/etc/polkit-1/rules.d"))
    }

    pub fn etc_polkit_xp_xray_restart_rule(&self) -> PathBuf {
        self.etc_polkit_rules_dir().join("90-xp-xray-restart.rules")
    }

    pub fn etc_doas_conf(&self) -> PathBuf {
        self.map_abs(Path::new("/etc/doas.conf"))
    }

    pub fn etc_xp_ops_cloudflare_dir(&self) -> PathBuf {
        self.map_abs(Path::new("/etc/xp-ops/cloudflare_tunnel"))
    }

    pub fn etc_xp_ops_deploy_dir(&self) -> PathBuf {
        self.map_abs(Path::new("/etc/xp-ops/deploy"))
    }

    pub fn etc_xp_ops_deploy_settings(&self) -> PathBuf {
        self.map_abs(Path::new("/etc/xp-ops/deploy/settings.json"))
    }

    pub fn etc_xp_ops_cloudflare_settings(&self) -> PathBuf {
        self.map_abs(Path::new("/etc/xp-ops/cloudflare_tunnel/settings.json"))
    }

    pub fn etc_xp_ops_cloudflare_token(&self) -> PathBuf {
        self.map_abs(Path::new("/etc/xp-ops/cloudflare_tunnel/api_token"))
    }

    pub fn systemd_unit_dir(&self) -> PathBuf {
        self.map_abs(Path::new("/etc/systemd/system"))
    }

    pub fn openrc_initd_dir(&self) -> PathBuf {
        self.map_abs(Path::new("/etc/init.d"))
    }

    pub fn openrc_confd_dir(&self) -> PathBuf {
        self.map_abs(Path::new("/etc/conf.d"))
    }

    pub fn usr_local_bin_xp(&self) -> PathBuf {
        self.map_abs(Path::new("/usr/local/bin/xp"))
    }

    pub fn usr_local_bin_xray(&self) -> PathBuf {
        self.map_abs(Path::new("/usr/local/bin/xray"))
    }
}
