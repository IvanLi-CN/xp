use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FallbackTarget {
    pub scheme: String,
    pub authority: String,
}

impl FallbackTarget {
    pub fn parse(raw: &str) -> Result<Self, String> {
        let (scheme, authority) = raw
            .split_once("://")
            .ok_or_else(|| "fallback target must include a scheme".to_string())?;
        if !matches!(scheme, "http" | "https" | "h2" | "h2c") {
            return Err(format!("unsupported fallback scheme: {scheme}"));
        }
        if authority.is_empty()
            || authority.contains('/')
            || authority.contains('?')
            || authority.contains('#')
            || authority.contains('@')
        {
            return Err("fallback authority must be a host and optional port".into());
        }
        let (host, port) = if let Some(bracketed) = authority.strip_prefix('[') {
            let (host, suffix) = bracketed
                .split_once(']')
                .ok_or("fallback IPv6 authority requires closing bracket")?;
            host.parse::<std::net::Ipv6Addr>()
                .map_err(|_| "fallback authority has an invalid IPv6 address")?;
            let port = if suffix.is_empty() {
                None
            } else {
                Some(
                    suffix
                        .strip_prefix(':')
                        .ok_or("invalid IPv6 authority suffix")?,
                )
            };
            (host, port)
        } else {
            if authority.contains(['[', ']']) || authority.matches(':').count() > 1 {
                return Err("fallback IPv6 authority must be bracketed".into());
            }
            match authority.split_once(':') {
                Some((host, port)) => (host, Some(port)),
                None => (authority, None),
            }
        };
        if host.is_empty()
            || host
                .chars()
                .any(|ch| ch.is_ascii_whitespace() || ch.is_control())
        {
            return Err("fallback authority has an invalid host".into());
        }
        if let Some(port) = port
            && (port.is_empty()
                || !port.bytes().all(|byte| byte.is_ascii_digit())
                || port.parse::<u16>().is_err())
        {
            return Err("fallback authority has an invalid port".into());
        }
        Ok(Self {
            scheme: scheme.to_owned(),
            authority: authority.to_owned(),
        })
    }

    pub fn is_allowed(&self, authorities: &BTreeSet<String>) -> bool {
        authorities.contains(&self.authority)
    }
}

pub fn clean_hop_by_hop_headers(headers: &mut Vec<(String, String)>) {
    const HOP_HEADERS: [&str; 8] = [
        "connection",
        "keep-alive",
        "proxy-authenticate",
        "proxy-authorization",
        "te",
        "trailer",
        "transfer-encoding",
        "upgrade",
    ];
    headers.retain(|(name, _)| !HOP_HEADERS.iter().any(|hop| name.eq_ignore_ascii_case(hop)));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_fixed_fallback_schemes() {
        assert!(FallbackTarget::parse("https://origin.example:443").is_ok());
        assert!(FallbackTarget::parse("h2c://127.0.0.1:8080").is_ok());
        assert!(FallbackTarget::parse("tcp://origin.example:443").is_err());
        assert!(FallbackTarget::parse("https://user@origin.example").is_err());
    }

    #[test]
    fn authority_allowlist_is_exact() {
        let target = FallbackTarget::parse("http://origin.example:80").unwrap();
        let allowed = BTreeSet::from(["origin.example:80".to_owned()]);
        assert!(target.is_allowed(&allowed));
        assert!(!target.is_allowed(&BTreeSet::from(["other.example:80".to_owned()])));
    }

    #[test]
    fn validates_ipv6_brackets_address_port_and_exact_allowlist() {
        for scheme in ["http", "https", "h2", "h2c"] {
            let target = FallbackTarget::parse(&format!("{scheme}://[::1]:8443")).unwrap();
            assert!(target.is_allowed(&BTreeSet::from(["[::1]:8443".into()])));
            assert!(!target.is_allowed(&BTreeSet::from(["[::1]:443".into()])));
        }
        for authority in [
            "::1:443",
            "[::1",
            "[::1]garbage",
            "[nope]:80",
            "[::1]:65536",
            "[::1]:",
            "[::1]:-1",
            "[::1]:80:90",
            "[::1] :80",
        ] {
            assert!(
                FallbackTarget::parse(&format!("https://{authority}")).is_err(),
                "{authority}"
            );
        }
    }

    #[test]
    fn removes_hop_by_hop_headers_case_insensitively() {
        let mut headers = vec![
            ("Connection".into(), "close".into()),
            ("Host".into(), "origin.example".into()),
            ("x-request-id".into(), "abc".into()),
            ("Upgrade".into(), "h2c".into()),
        ];
        clean_hop_by_hop_headers(&mut headers);
        assert_eq!(headers.len(), 2);
        assert_eq!(headers[0].0, "Host");
        assert_eq!(headers[1].0, "x-request-id");
    }
}
