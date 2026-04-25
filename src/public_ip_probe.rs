use std::{
    net::{IpAddr, SocketAddr, ToSocketAddrs},
    time::Duration,
};

use reqwest::Url;

pub const DEFAULT_TRACE_URL: &str = "https://cloudflare.com/cdn-cgi/trace";
const DEFAULT_PROBE_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicIpAddressFamily {
    Ipv4,
    Ipv6,
}

impl PublicIpAddressFamily {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ipv4 => "ipv4",
            Self::Ipv6 => "ipv6",
        }
    }

    fn matches_ip(self, ip: IpAddr) -> bool {
        matches!(
            (self, ip),
            (Self::Ipv4, IpAddr::V4(_)) | (Self::Ipv6, IpAddr::V6(_))
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PublicIpProbeOutcome {
    Available(IpAddr),
    Unknown(String),
    MissingCandidate(String),
}

pub async fn probe_public_ip(url: &str, family: PublicIpAddressFamily) -> PublicIpProbeOutcome {
    let parsed = match Url::parse(url) {
        Ok(url) => url,
        Err(err) => return PublicIpProbeOutcome::Unknown(format!("invalid url {url}: {err}")),
    };
    let Some(host) = parsed.host_str().map(ToOwned::to_owned) else {
        return PublicIpProbeOutcome::Unknown(format!("invalid url host for {url}"));
    };
    let port = parsed.port_or_known_default().unwrap_or(443);

    let resolved = if let Ok(ip) = host.parse::<IpAddr>() {
        if !family.matches_ip(ip) {
            return PublicIpProbeOutcome::MissingCandidate(format!(
                "configured {} probe URL is pinned to the other address family",
                family.label()
            ));
        }
        vec![SocketAddr::new(ip, port)]
    } else {
        let host_clone = host.clone();
        let lookup = tokio::task::spawn_blocking(move || -> Result<Vec<SocketAddr>, String> {
            (host_clone.as_str(), port)
                .to_socket_addrs()
                .map(|iter| iter.collect())
                .map_err(|err| err.to_string())
        })
        .await;
        let addresses = match lookup {
            Ok(Ok(addrs)) => addrs,
            Ok(Err(err)) => {
                return PublicIpProbeOutcome::Unknown(format!("dns lookup failed: {err}"));
            }
            Err(err) => {
                return PublicIpProbeOutcome::Unknown(format!("dns lookup task failed: {err}"));
            }
        };
        let filtered: Vec<SocketAddr> = addresses
            .into_iter()
            .filter(|addr| family.matches_ip(addr.ip()))
            .collect();
        if filtered.is_empty() {
            return PublicIpProbeOutcome::MissingCandidate(format!(
                "probe hostname resolved without any {} target",
                family.label()
            ));
        }
        filtered
    };

    let mut builder = reqwest::Client::builder().timeout(DEFAULT_PROBE_TIMEOUT);
    if parsed
        .host_str()
        .is_some_and(|value| value.parse::<IpAddr>().is_err())
    {
        builder = builder.resolve_to_addrs(&host, &resolved);
    }
    let client = match builder.build() {
        Ok(client) => client,
        Err(err) => return PublicIpProbeOutcome::Unknown(format!("build probe client: {err}")),
    };

    let response = match client.get(parsed.clone()).send().await {
        Ok(response) => response,
        Err(err) => return classify_probe_error(err, family),
    };
    if !response.status().is_success() {
        return PublicIpProbeOutcome::Unknown(format!(
            "unexpected HTTP status {}",
            response.status()
        ));
    }
    let body = match response.text().await {
        Ok(body) => body,
        Err(err) => return PublicIpProbeOutcome::Unknown(format!("read body failed: {err}")),
    };
    match parse_probe_ip(&body, family) {
        Ok(ip) => PublicIpProbeOutcome::Available(ip),
        Err(err) => PublicIpProbeOutcome::Unknown(err),
    }
}

fn parse_probe_ip(body: &str, family: PublicIpAddressFamily) -> Result<IpAddr, String> {
    let trimmed = body.trim();
    if let Ok(ip) = trimmed.parse::<IpAddr>() {
        return if family.matches_ip(ip) {
            Ok(ip)
        } else {
            Err(format!(
                "probe returned {ip}, which is not {}",
                family.label()
            ))
        };
    }

    for line in trimmed.lines() {
        if let Some(rest) = line.strip_prefix("ip=") {
            let ip = rest
                .trim()
                .parse::<IpAddr>()
                .map_err(|err| format!("parse trace ip: {err}"))?;
            if family.matches_ip(ip) {
                return Ok(ip);
            }
            return Err(format!(
                "probe returned {ip}, which is not {}",
                family.label()
            ));
        }
    }

    Err("probe response did not contain ip=... or plain IP body".to_string())
}

fn classify_probe_error(
    err: reqwest::Error,
    family: PublicIpAddressFamily,
) -> PublicIpProbeOutcome {
    if err.is_timeout() {
        return PublicIpProbeOutcome::Unknown("timeout".to_string());
    }

    let text = err.to_string().to_ascii_lowercase();
    if text.contains("network is unreachable")
        || text.contains("network unreachable")
        || text.contains("address family not supported")
        || text.contains("no route to host")
        || text.contains("cannot assign requested address")
        || text.contains("requested address is not valid")
    {
        return PublicIpProbeOutcome::MissingCandidate(format!(
            "{} path is unavailable: {}",
            family.label(),
            err
        ));
    }

    PublicIpProbeOutcome::Unknown(err.to_string())
}

#[cfg(test)]
mod tests {
    use std::net::IpAddr;

    use super::*;

    #[test]
    fn trace_parser_accepts_plain_ip_and_trace_ip() {
        assert_eq!(
            parse_probe_ip("1.2.3.4\n", PublicIpAddressFamily::Ipv4).unwrap(),
            "1.2.3.4".parse::<IpAddr>().unwrap()
        );
        assert_eq!(
            parse_probe_ip(
                "fl=29f\nip=2001:db8::1\nts=1\n",
                PublicIpAddressFamily::Ipv6
            )
            .unwrap(),
            "2001:db8::1".parse::<IpAddr>().unwrap()
        );
    }

    #[test]
    fn trace_parser_rejects_wrong_family() {
        let err = parse_probe_ip("ip=1.2.3.4\n", PublicIpAddressFamily::Ipv6).unwrap_err();
        assert!(err.contains("not ipv6"));
    }
}
