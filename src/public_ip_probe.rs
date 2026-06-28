use std::{
    error::Error,
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
    classify_probe_failure(
        err.is_timeout(),
        err.to_string(),
        probe_error_texts(&err),
        family,
    )
}

fn classify_probe_failure(
    is_timeout: bool,
    display: String,
    error_texts: Vec<String>,
    family: PublicIpAddressFamily,
) -> PublicIpProbeOutcome {
    if is_timeout {
        return PublicIpProbeOutcome::Unknown("timeout".to_string());
    }

    if error_texts
        .iter()
        .any(|text| is_missing_candidate_error_text(text))
    {
        return PublicIpProbeOutcome::MissingCandidate(format!(
            "{} path is unavailable: {}",
            family.label(),
            display
        ));
    }

    PublicIpProbeOutcome::Unknown(display)
}

fn probe_error_texts(err: &(dyn Error + 'static)) -> Vec<String> {
    let mut texts = vec![err.to_string().to_ascii_lowercase()];
    let mut current = err.source();
    while let Some(source) = current {
        texts.push(source.to_string().to_ascii_lowercase());
        current = source.source();
    }
    texts
}

fn is_missing_candidate_error_text(text: &str) -> bool {
    let text = text.to_ascii_lowercase();
    text.contains("network is unreachable")
        || text.contains("network unreachable")
        || text.contains("address family not supported")
        || text.contains("no route to host")
        || text.contains("cannot assign requested address")
        || text.contains("can't assign requested address")
        || text.contains("requested address is not valid")
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

    #[test]
    fn missing_candidate_detection_checks_nested_error_text() {
        assert!(is_missing_candidate_error_text(
            "client error: tcp connect error: Network is unreachable (os error 101)"
        ));
        assert!(is_missing_candidate_error_text(
            "connect failed: No route to host"
        ));
        assert!(is_missing_candidate_error_text(
            "connect failed: Cannot assign requested address"
        ));
        assert!(is_missing_candidate_error_text(
            "connect failed: Can't assign requested address (os error 49)"
        ));
    }

    #[test]
    fn missing_candidate_detection_rejects_transient_connect_failures() {
        assert!(!is_missing_candidate_error_text(
            "error sending request for url (https://cloudflare.com/cdn-cgi/trace)"
        ));
        assert!(!is_missing_candidate_error_text(
            "client error: tcp connect error: Connection refused (os error 111)"
        ));
        assert!(!is_missing_candidate_error_text("timeout"));
    }

    #[test]
    fn probe_failure_classifier_uses_source_chain_without_treating_timeouts_as_missing() {
        assert_eq!(
            classify_probe_failure(
                false,
                "error sending request for url (https://cloudflare.com/cdn-cgi/trace)"
                    .to_string(),
                vec![
                    "error sending request for url (https://cloudflare.com/cdn-cgi/trace)"
                        .to_string(),
                    "client error: tcp connect error: network is unreachable (os error 101)"
                        .to_string(),
                ],
                PublicIpAddressFamily::Ipv6,
            ),
            PublicIpProbeOutcome::MissingCandidate(
                "ipv6 path is unavailable: error sending request for url (https://cloudflare.com/cdn-cgi/trace)"
                    .to_string()
            )
        );

        assert_eq!(
            classify_probe_failure(
                true,
                "operation timed out".to_string(),
                vec!["network is unreachable".to_string()],
                PublicIpAddressFamily::Ipv6,
            ),
            PublicIpProbeOutcome::Unknown("timeout".to_string())
        );
    }
}
