#[derive(Debug, Clone)]
pub struct ManagedRuntimeTargets {
    pub xray_systemd_unit: String,
    pub xray_openrc_service: String,
    pub cloudflared_systemd_unit: String,
    pub cloudflared_openrc_service: String,
}

impl Default for ManagedRuntimeTargets {
    fn default() -> Self {
        Self {
            xray_systemd_unit: "xray.service".to_string(),
            xray_openrc_service: "xray".to_string(),
            cloudflared_systemd_unit: "cloudflared.service".to_string(),
            cloudflared_openrc_service: "cloudflared".to_string(),
        }
    }
}

impl From<&crate::config::Config> for ManagedRuntimeTargets {
    fn from(config: &crate::config::Config) -> Self {
        Self {
            xray_systemd_unit: config.xray_systemd_unit.clone(),
            xray_openrc_service: config.xray_openrc_service.clone(),
            cloudflared_systemd_unit: config.cloudflared_systemd_unit.clone(),
            cloudflared_openrc_service: config.cloudflared_openrc_service.clone(),
        }
    }
}
