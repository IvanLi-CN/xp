use super::catalog;

pub fn primary_server_name() -> &'static str {
    &catalog().hosts.server_primary
}

pub fn secondary_server_name() -> &'static str {
    &catalog().hosts.server_secondary
}

pub fn loopback_39043_url() -> &'static str {
    &catalog().urls.loopback_39043
}

pub fn public_fallback_url() -> &'static str {
    &catalog().urls.public_fallback
}

pub fn catch_all_service() -> &'static str {
    &catalog().urls.catch_all_service
}

pub fn loopback_39043_address() -> &'static str {
    &catalog().addresses.loopback_39043
}

pub fn loopback_49043_address() -> &'static str {
    &catalog().addresses.loopback_49043
}
