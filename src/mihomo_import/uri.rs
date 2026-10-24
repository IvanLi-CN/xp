use super::{Candidate, ImportError, MAX_CANDIDATES, meaningful_lines};
use base64::Engine as _;
use reqwest::Url;
use serde_yaml::{Mapping, Value};
use std::collections::BTreeMap;

pub(super) fn parse_raw_candidates(source: &str) -> Result<Vec<Candidate>, ImportError> {
    let mut candidates = Vec::new();
    for line in meaningful_lines(source) {
        if candidates.len() == MAX_CANDIDATES {
            return Err(ImportError::too_large(format!(
                "source contains more than {MAX_CANDIDATES} candidates"
            )));
        }
        candidates.push({
            let line = line.strip_prefix("- ").map(str::trim).unwrap_or(line);
            let scheme = line
                .split_once("://")
                .map(|(scheme, _)| scheme.trim().to_ascii_lowercase());
            match parse_uri_proxy(line) {
                Ok((proxy, name, scheme, warnings)) => Candidate {
                    scheme: Some(scheme),
                    name,
                    proxy: Some(proxy),
                    warnings,
                    failure: None,
                },
                Err(reason) => Candidate {
                    scheme,
                    name: None,
                    proxy: None,
                    warnings: Vec::new(),
                    failure: Some(reason),
                },
            }
        });
    }
    Ok(candidates)
}

fn parse_uri_proxy(source: &str) -> Result<(Value, Option<String>, String, Vec<String>), String> {
    let url = Url::parse(source).map_err(|_| "URI is malformed".to_owned())?;
    let scheme = url.scheme().to_ascii_lowercase();
    let fragment_name = parse_fragment_name(&url)?;
    let opaque_payload = source
        .split_once("://")
        .map(|(_, payload)| payload)
        .unwrap_or_default()
        .split(['#', '?'])
        .next()
        .unwrap_or_default();
    match scheme.as_str() {
        "vless" => parse_vless(url, fragment_name),
        "ss" => parse_ss(url, fragment_name, opaque_payload),
        "vmess" => parse_vmess(url, fragment_name, opaque_payload),
        "trojan" => parse_trojan(url, fragment_name),
        "hysteria2" | "hy2" => parse_hysteria2(url, fragment_name, source),
        "tuic" => parse_tuic(url, fragment_name),
        _ => Err("unsupported scheme".to_owned()),
    }
}

fn parse_vless(
    url: Url,
    name: Option<String>,
) -> Result<(Value, Option<String>, String, Vec<String>), String> {
    let uuid = decode_component(url.username())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "vless uuid is required".to_owned())?;
    let (server, port) = host_port(&url, "vless")?;
    let query = query_map(&url);
    let network = query.get("type").map(String::as_str).unwrap_or("tcp");
    if !supported_network(network) {
        return Err("vless transport is unsupported".to_owned());
    }
    let security = query.get("security").map(String::as_str).unwrap_or("none");
    if !matches!(security, "none" | "tls" | "reality") {
        return Err("vless security is unsupported".to_owned());
    }
    let mut map = base_proxy("vless", &server, port);
    insert_string(&mut map, "uuid", uuid);
    insert_string(&mut map, "network", network);
    insert_bool(&mut map, "udp", true);
    insert_bool(&mut map, "tls", security != "none");
    if let Some(encryption) = query.get("encryption") {
        insert_string(&mut map, "encryption", encryption.clone());
    }
    if let Some(flow) = query.get("flow").filter(|value| !value.is_empty()) {
        insert_string(&mut map, "flow", flow.clone());
    }
    if let Some(sni) = query.get("sni").or_else(|| query.get("servername")) {
        insert_string(&mut map, "servername", sni.clone());
    }
    if let Some(fp) = query.get("fp").or_else(|| query.get("fingerprint")) {
        insert_string(&mut map, "client-fingerprint", fp.clone());
    }
    if let Some(alpn) = query.get("alpn") {
        insert_alpn(&mut map, alpn);
    }
    let mut warnings = Vec::new();
    if security == "reality" {
        let public_key = query
            .get("pbk")
            .filter(|value| !value.is_empty())
            .ok_or_else(|| "vless reality public key is required".to_owned())?;
        let short_id = query
            .get("sid")
            .ok_or_else(|| "vless reality short id is required".to_owned())?;
        let mut reality = Mapping::new();
        insert_string(&mut reality, "public-key", public_key.clone());
        insert_string(&mut reality, "short-id", short_id.clone());
        map.insert(
            Value::String("reality-opts".to_owned()),
            Value::Mapping(reality),
        );
    }
    if network == "ws" {
        add_ws_options(&mut map, &query);
    } else if network == "grpc" {
        add_grpc_options(&mut map, &query);
    }
    if query.contains_key("packet-encoding") {
        warnings.push("ignored_optional_parameter:packet-encoding".to_owned());
    }
    Ok((
        finish_proxy(map, name.clone()),
        name,
        "vless".to_owned(),
        warnings,
    ))
}

fn parse_ss(
    url: Url,
    name: Option<String>,
    opaque_payload: &str,
) -> Result<(Value, Option<String>, String, Vec<String>), String> {
    let (method, password, server, port) = parse_ss_parts(&url, opaque_payload)?;
    let mut map = base_proxy("ss", &server, port);
    insert_string(&mut map, "cipher", method);
    insert_string(&mut map, "password", password);
    insert_bool(&mut map, "udp", true);
    if url.query_pairs().any(|(key, _)| key == "plugin") {
        return Err("ss plugin transport is unsupported".to_owned());
    }
    Ok((
        finish_proxy(map, name.clone()),
        name,
        "ss".to_owned(),
        Vec::new(),
    ))
}

fn parse_ss_parts(
    url: &Url,
    opaque_payload: &str,
) -> Result<(String, String, String, u16), String> {
    let direct_user = decode_component(url.username()).unwrap_or_default();
    let direct_password = url.password().and_then(decode_component);
    let explicit_port = url.port().or_else(|| {
        opaque_payload
            .rsplit_once('@')
            .and_then(|(_, authority)| explicit_authority_port(authority))
    });
    if let (Some(server), Some(port), Some(password)) =
        (normalized_host(url), explicit_port, direct_password)
    {
        if direct_user.trim().is_empty() || password.trim().is_empty() || port == 0 {
            return Err("ss port is invalid".to_owned());
        }
        return Ok((direct_user, password, server, port));
    }

    if let (Some(server), Some(port)) = (normalized_host(url), explicit_port)
        && let Some(decoded) = decode_base64_bytes(&direct_user)
        && let Ok(decoded) = String::from_utf8(decoded)
        && let Some((method, password)) = decoded.split_once(':')
    {
        if method.trim().is_empty() || password.trim().is_empty() || port == 0 {
            return Err("ss method and password are required".to_owned());
        }
        return Ok((method.to_owned(), password.to_owned(), server, port));
    }

    let encoded = if !opaque_payload.is_empty() {
        opaque_payload.to_owned()
    } else if !direct_user.is_empty() && url.host_str().is_some() {
        direct_user
    } else {
        url.host_str()
            .or_else(|| url.path().trim_start_matches('/').split('/').next())
            .unwrap_or_default()
            .to_owned()
    };
    let decoded =
        decode_base64_bytes(&encoded).ok_or_else(|| "ss payload is invalid".to_owned())?;
    let decoded = String::from_utf8(decoded).map_err(|_| "ss payload is invalid".to_owned())?;
    let (userinfo, authority) = decoded
        .rsplit_once('@')
        .ok_or_else(|| "ss payload is missing server".to_owned())?;
    let (method, password) = userinfo
        .split_once(':')
        .ok_or_else(|| "ss payload is missing password".to_owned())?;
    if method.trim().is_empty() || password.trim().is_empty() {
        return Err("ss method and password are required".to_owned());
    }
    let authority_url = Url::parse(&format!("http://{authority}"))
        .map_err(|_| "ss payload server is invalid".to_owned())?;
    let server = normalized_host(&authority_url)
        .ok_or_else(|| "ss payload server is required".to_owned())?;
    let port = explicit_authority_port(authority)
        .ok_or_else(|| "ss payload port is required".to_owned())?;
    Ok((method.to_owned(), password.to_owned(), server, port))
}

fn explicit_authority_port(authority: &str) -> Option<u16> {
    let port = if let Some((_, suffix)) = authority.rsplit_once(']') {
        suffix.strip_prefix(':')
    } else {
        authority.rsplit_once(':').map(|(_, suffix)| suffix)
    }?;
    port.parse::<u16>().ok().filter(|port| *port > 0)
}

fn parse_vmess(
    url: Url,
    fragment_name: Option<String>,
    opaque_payload: &str,
) -> Result<(Value, Option<String>, String, Vec<String>), String> {
    let encoded = if !opaque_payload.is_empty() {
        opaque_payload.to_owned()
    } else if !url.path().trim_matches('/').is_empty() {
        url.path().trim_matches('/').to_owned()
    } else {
        url.host_str().unwrap_or_default().to_owned()
    };
    let decoded =
        decode_base64_bytes(&encoded).ok_or_else(|| "vmess payload is invalid".to_owned())?;
    let payload: serde_json::Value = serde_json::from_slice(&decoded)
        .map_err(|_| "vmess payload is not valid JSON".to_owned())?;
    let server = json_string(&payload, "add")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "vmess server is required".to_owned())?;
    let port = json_port(&payload, "port").ok_or_else(|| "vmess port is required".to_owned())?;
    let uuid = json_string(&payload, "id")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "vmess uuid is required".to_owned())?;
    let network = json_string(&payload, "net").unwrap_or_else(|| "tcp".to_owned());
    if !supported_network(&network) {
        return Err("vmess transport is unsupported".to_owned());
    }
    let vmess_http =
        match json_string(&payload, "type").filter(|value| !value.is_empty() && value != "none") {
            Some(value) if value == "http" => true,
            Some(_) => return Err("vmess transport is unsupported".to_owned()),
            None => false,
        };

    let mut map = base_proxy("vmess", &server, port);
    insert_string(&mut map, "uuid", uuid);
    insert_string(
        &mut map,
        "network",
        if vmess_http { "http" } else { network.as_str() },
    );
    insert_string(
        &mut map,
        "cipher",
        json_string(&payload, "scy").unwrap_or_else(|| "auto".to_owned()),
    );
    insert_bool(&mut map, "udp", true);
    if payload.get("aid").is_some() {
        let alter_id =
            json_u64(&payload, "aid").ok_or_else(|| "vmess alterId is invalid".to_owned())?;
        insert_number(&mut map, "alterId", alter_id);
    }
    let tls = json_string(&payload, "tls")
        .map(|value| value == "tls" || value == "true")
        .or_else(|| payload.get("tls").and_then(serde_json::Value::as_bool))
        .unwrap_or(false);
    insert_bool(&mut map, "tls", tls);
    if let Some(insecure) = json_bool(&payload, "insecure") {
        insert_bool(&mut map, "skip-cert-verify", insecure);
    }
    if let Some(sni) = json_string(&payload, "sni").or_else(|| json_string(&payload, "host")) {
        insert_string(&mut map, "servername", sni);
    }
    if network == "ws" {
        let mut ws = Mapping::new();
        if let Some(path) = json_string(&payload, "path") {
            insert_string(&mut ws, "path", path);
        }
        if let Some(host) = json_string(&payload, "host") {
            let mut headers = Mapping::new();
            insert_string(&mut headers, "Host", host);
            ws.insert(Value::String("headers".to_owned()), Value::Mapping(headers));
        }
        if !ws.is_empty() {
            map.insert(Value::String("ws-opts".to_owned()), Value::Mapping(ws));
        }
    } else if network == "grpc"
        && let Some(service) = json_string(&payload, "path")
    {
        let mut grpc = Mapping::new();
        insert_string(&mut grpc, "grpc-service-name", service);
        map.insert(Value::String("grpc-opts".to_owned()), Value::Mapping(grpc));
    } else if vmess_http {
        add_vmess_http_options(&mut map, &payload);
    }
    let payload_name = safe_json_name(json_string(&payload, "ps"))?;
    let name = fragment_name.or(payload_name);
    Ok((
        finish_proxy(map, name.clone()),
        name,
        "vmess".to_owned(),
        Vec::new(),
    ))
}

fn parse_trojan(
    url: Url,
    name: Option<String>,
) -> Result<(Value, Option<String>, String, Vec<String>), String> {
    let password = decode_component(url.username())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "trojan password is required".to_owned())?;
    let (server, port) = host_port(&url, "trojan")?;
    let query = query_map(&url);
    let network = query.get("type").map(String::as_str).unwrap_or("tcp");
    if !supported_network(network) {
        return Err("trojan transport is unsupported".to_owned());
    }
    let mut map = base_proxy("trojan", &server, port);
    insert_string(&mut map, "password", password);
    insert_bool(&mut map, "udp", true);
    insert_bool(&mut map, "tls", true);
    insert_string(&mut map, "network", network);
    if let Some(sni) = query.get("sni").or_else(|| query.get("peer")) {
        insert_string(&mut map, "sni", sni.clone());
    }
    if let Some(fp) = query.get("fp") {
        insert_string(&mut map, "client-fingerprint", fp.clone());
    }
    if network == "ws" {
        add_ws_options(&mut map, &query);
    } else if network == "grpc" {
        add_grpc_options(&mut map, &query);
    }
    if query
        .get("allowInsecure")
        .or_else(|| query.get("allow-insecure"))
        .is_some()
    {
        insert_bool(
            &mut map,
            "skip-cert-verify",
            query_bool(&query, "allowInsecure")
                .or_else(|| query_bool(&query, "allow-insecure"))
                .unwrap_or(false),
        );
    }
    Ok((
        finish_proxy(map, name.clone()),
        name,
        "trojan".to_owned(),
        Vec::new(),
    ))
}

fn parse_hysteria2(
    url: Url,
    name: Option<String>,
    source: &str,
) -> Result<(Value, Option<String>, String, Vec<String>), String> {
    let userinfo_has_colon = source
        .split_once("://")
        .and_then(|(_, remainder)| remainder.split_once('@'))
        .is_some_and(|(userinfo, _)| userinfo.contains(':'));
    let auth = match url.password() {
        Some(raw_password) => {
            let username = decode_component(url.username())
                .ok_or_else(|| "hysteria2 authentication is invalid".to_owned())?;
            let password = decode_component(raw_password)
                .ok_or_else(|| "hysteria2 authentication is invalid".to_owned())?;
            if username.is_empty() && password.is_empty() {
                return Err("hysteria2 password is required".to_owned());
            }
            format!("{username}:{password}")
        }
        None if userinfo_has_colon => {
            let username = decode_component(url.username())
                .ok_or_else(|| "hysteria2 authentication is invalid".to_owned())?;
            if username.is_empty() {
                return Err("hysteria2 password is required".to_owned());
            }
            format!("{username}:")
        }
        None => decode_component(url.username())
            .filter(|value| !value.is_empty())
            .ok_or_else(|| "hysteria2 password is required".to_owned())?,
    };
    let (server, port) = host_port(&url, "hysteria2")?;
    let query = query_map(&url);
    let mut map = base_proxy("hysteria2", &server, port);
    insert_string(&mut map, "password", auth);
    if let Some(sni) = query.get("sni") {
        insert_string(&mut map, "sni", sni.clone());
    }
    if let Some(insecure) = query_bool(&query, "insecure") {
        insert_bool(&mut map, "skip-cert-verify", insecure);
    }
    if let Some(pin) = query
        .get("pinSHA256")
        .or_else(|| query.get("pin-sha256"))
        .filter(|value| !value.is_empty())
    {
        insert_string(&mut map, "fingerprint", pin.clone());
    }
    if let Some(obfs) = query.get("obfs") {
        if obfs != "salamander" {
            return Err("hysteria2 obfuscation is unsupported".to_owned());
        }
        insert_string(&mut map, "obfs", obfs.clone());
        let obfs_password = query
            .get("obfs-password")
            .filter(|value| !value.is_empty())
            .ok_or_else(|| "hysteria2 obfs password is required".to_owned())?;
        insert_string(&mut map, "obfs-password", obfs_password.clone());
    }
    Ok((
        finish_proxy(map, name.clone()),
        name,
        "hysteria2".to_owned(),
        Vec::new(),
    ))
}

fn parse_tuic(
    url: Url,
    name: Option<String>,
) -> Result<(Value, Option<String>, String, Vec<String>), String> {
    let uuid = decode_component(url.username())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "tuic uuid is required".to_owned())?;
    let password = url
        .password()
        .and_then(decode_component)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "tuic password is required".to_owned())?;
    let (server, port) = host_port(&url, "tuic")?;
    let query = query_map(&url);
    let mut map = base_proxy("tuic", &server, port);
    insert_string(&mut map, "uuid", uuid);
    insert_string(&mut map, "password", password);
    if let Some(sni) = query.get("sni") {
        insert_string(&mut map, "sni", sni.clone());
    }
    for (query_key, map_key) in [
        ("congestion_control", "congestion-controller"),
        ("udp_relay_mode", "udp-relay-mode"),
        ("heartbeat", "heartbeat-interval"),
    ] {
        if let Some(value) = query.get(query_key) {
            insert_string(&mut map, map_key, value.clone());
        }
    }
    if let Some(value) = query_bool(&query, "zero_rtt") {
        insert_bool(&mut map, "reduce-rtt", value);
    }
    if let Some(value) = query_bool(&query, "disable_sni") {
        insert_bool(&mut map, "disable-sni", value);
    }
    if let Some(value) = query_bool(&query, "insecure")
        .or_else(|| query_bool(&query, "allow_insecure"))
        .or_else(|| query_bool(&query, "allow-insecure"))
    {
        insert_bool(&mut map, "skip-cert-verify", value);
    }
    if let Some(alpn) = query.get("alpn") {
        insert_alpn(&mut map, alpn);
    }
    Ok((
        finish_proxy(map, name.clone()),
        name,
        "tuic".to_owned(),
        Vec::new(),
    ))
}

fn host_port(url: &Url, scheme: &str) -> Result<(String, u16), String> {
    let server = normalized_host(url).ok_or_else(|| format!("{scheme} server is required"))?;
    let port = url
        .port()
        .or_else(|| matches!(scheme, "hysteria2" | "hy2").then_some(443));
    let port = port
        .filter(|port| *port > 0)
        .ok_or_else(|| format!("{scheme} port is required"))?;
    Ok((server, port))
}

fn supported_network(network: &str) -> bool {
    matches!(network, "tcp" | "ws" | "grpc")
}

fn normalized_host(url: &Url) -> Option<String> {
    let host = url.host_str()?.trim();
    if host.is_empty() {
        return None;
    }
    Some(
        host.strip_prefix('[')
            .and_then(|value| value.strip_suffix(']'))
            .unwrap_or(host)
            .to_owned(),
    )
}

fn query_map(url: &Url) -> BTreeMap<String, String> {
    url.query_pairs()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect()
}

fn query_bool(query: &BTreeMap<String, String>, key: &str) -> Option<bool> {
    query
        .get(key)
        .and_then(|value| match value.to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" => Some(true),
            "0" | "false" | "no" => Some(false),
            _ => None,
        })
}

fn base_proxy(proxy_type: &str, server: &str, port: u16) -> Mapping {
    let mut map = Mapping::new();
    insert_string(&mut map, "type", proxy_type);
    insert_string(&mut map, "server", server);
    map.insert(
        Value::String("port".to_owned()),
        Value::Number(serde_yaml::Number::from(port)),
    );
    map
}

fn finish_proxy(map: Mapping, name: Option<String>) -> Value {
    let name = name.unwrap_or_default();
    let mut output = Mapping::new();
    if !name.is_empty() {
        insert_string(&mut output, "name", name);
    }
    for (key, value) in map {
        output.insert(key, value);
    }
    Value::Mapping(output)
}

pub(super) fn insert_string(map: &mut Mapping, key: &str, value: impl Into<String>) {
    map.insert(Value::String(key.to_owned()), Value::String(value.into()));
}

fn insert_bool(map: &mut Mapping, key: &str, value: bool) {
    map.insert(Value::String(key.to_owned()), Value::Bool(value));
}

fn insert_alpn(map: &mut Mapping, alpn: &str) {
    let values = alpn
        .split([',', ';'])
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| Value::String(value.to_owned()))
        .collect::<Vec<_>>();
    if !values.is_empty() {
        map.insert(Value::String("alpn".to_owned()), Value::Sequence(values));
    }
}

fn add_ws_options(map: &mut Mapping, query: &BTreeMap<String, String>) {
    let mut ws = Mapping::new();
    if let Some(path) = query.get("path") {
        insert_string(&mut ws, "path", path.clone());
    }
    if let Some(host) = query.get("host") {
        let mut headers = Mapping::new();
        insert_string(&mut headers, "Host", host.clone());
        ws.insert(Value::String("headers".to_owned()), Value::Mapping(headers));
    }
    if !ws.is_empty() {
        map.insert(Value::String("ws-opts".to_owned()), Value::Mapping(ws));
    }
}

fn add_grpc_options(map: &mut Mapping, query: &BTreeMap<String, String>) {
    if let Some(service) = query
        .get("serviceName")
        .or_else(|| query.get("service-name"))
    {
        let mut grpc = Mapping::new();
        insert_string(&mut grpc, "grpc-service-name", service.clone());
        map.insert(Value::String("grpc-opts".to_owned()), Value::Mapping(grpc));
    }
}

fn add_vmess_http_options(map: &mut Mapping, payload: &serde_json::Value) {
    let mut http = Mapping::new();
    if let Some(path) = json_string(payload, "path") {
        http.insert(
            Value::String("path".to_owned()),
            Value::Sequence(vec![Value::String(path)]),
        );
    }
    if let Some(host) = json_string(payload, "host") {
        let mut headers = Mapping::new();
        headers.insert(
            Value::String("Host".to_owned()),
            Value::Sequence(
                host.split(',')
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(|value| Value::String(value.to_owned()))
                    .collect(),
            ),
        );
        http.insert(Value::String("headers".to_owned()), Value::Mapping(headers));
    }
    if !http.is_empty() {
        map.insert(Value::String("http-opts".to_owned()), Value::Mapping(http));
    }
}

fn decode_component(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return None;
            }
            let high = hex_digit(bytes[index + 1])?;
            let low = hex_digit(bytes[index + 2])?;
            output.push(high * 16 + low);
            index += 3;
        } else {
            output.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(output).ok()
}

fn parse_fragment_name(url: &Url) -> Result<Option<String>, String> {
    let Some(fragment) = url.fragment() else {
        return Ok(None);
    };
    let name = decode_component(fragment).ok_or_else(|| "proxy name is invalid".to_owned())?;
    if name.trim().is_empty() {
        return Ok(None);
    }
    safe_name(&name)
        .map(Some)
        .ok_or_else(|| "proxy name is invalid".to_owned())
}

fn safe_json_name(value: Option<String>) -> Result<Option<String>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.trim().is_empty() {
        return Ok(None);
    }
    safe_name(&value)
        .map(Some)
        .ok_or_else(|| "proxy name is invalid".to_owned())
}

fn hex_digit(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn decode_base64_bytes(value: &str) -> Option<Vec<u8>> {
    let compact: String = value
        .chars()
        .filter(|ch| !ch.is_ascii_whitespace())
        .collect();
    let mut candidates = vec![compact.clone()];
    let rem = compact.len() % 4;
    if rem != 0 {
        candidates.push(format!("{compact}{}", "=".repeat(4 - rem)));
    }
    for candidate in candidates {
        for engine in [
            base64::engine::general_purpose::STANDARD,
            base64::engine::general_purpose::URL_SAFE,
        ] {
            if let Ok(bytes) = engine.decode(&candidate) {
                return Some(bytes);
            }
        }
    }
    None
}

fn json_string(value: &serde_json::Value, key: &str) -> Option<String> {
    value.get(key).and_then(|value| {
        value
            .as_str()
            .map(str::to_owned)
            .or_else(|| value.as_i64().map(|number| number.to_string()))
    })
}

fn json_port(value: &serde_json::Value, key: &str) -> Option<u16> {
    value.get(key).and_then(|value| {
        value
            .as_u64()
            .and_then(|number| u16::try_from(number).ok())
            .filter(|port| *port > 0)
            .or_else(|| value.as_str()?.parse().ok().filter(|port| *port > 0))
    })
}

fn json_u64(value: &serde_json::Value, key: &str) -> Option<u64> {
    value
        .get(key)
        .and_then(|value| value.as_u64().or_else(|| value.as_str()?.parse().ok()))
}

fn json_bool(value: &serde_json::Value, key: &str) -> Option<bool> {
    value.get(key).and_then(|value| {
        value.as_bool().or_else(|| {
            value
                .as_str()
                .and_then(|value| match value.to_ascii_lowercase().as_str() {
                    "1" | "true" | "yes" => Some(true),
                    "0" | "false" | "no" => Some(false),
                    _ => None,
                })
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_vless_encryption_and_explicit_empty_reality_short_id() {
        let url = Url::parse(concat!(
            "vless://00000000-0000-0000-0000-000000000001@example.com:443?",
            "encryption=mlkem768x25519plus.native%2Fxorpub%2Ftoken&",
            "security=reality&pbk=public-key&sid=#reality"
        ))
        .expect("URL");
        let (proxy, _, _, _) = parse_vless(url, Some("reality".to_owned())).expect("proxy");
        assert_eq!(
            proxy["encryption"].as_str(),
            Some("mlkem768x25519plus.native/xorpub/token")
        );
        assert_eq!(proxy["reality-opts"]["short-id"].as_str(), Some(""));
    }

    #[test]
    fn preserves_vmess_http_transport_options() {
        let payload = serde_json::json!({
            "v": "2",
            "ps": "vmess-http",
            "add": "vmess.example",
            "port": "443",
            "id": "00000000-0000-0000-0000-000000000001",
            "net": "tcp",
            "type": "http",
            "path": "/video",
            "host": "cdn.example, cdn-2.example",
            "insecure": "1",
        });
        let encoded = base64::engine::general_purpose::STANDARD.encode(payload.to_string());
        let url = Url::parse(&format!("vmess://{encoded}")).expect("URL");
        let (proxy, _, _, _) = parse_vmess(url, None, &encoded).expect("proxy");
        assert_eq!(proxy["network"].as_str(), Some("http"));
        assert_eq!(proxy["http-opts"]["path"][0].as_str(), Some("/video"));
        assert_eq!(
            proxy["http-opts"]["headers"]["Host"][0].as_str(),
            Some("cdn.example")
        );
        assert_eq!(
            proxy["http-opts"]["headers"]["Host"][1].as_str(),
            Some("cdn-2.example")
        );
        assert_eq!(proxy["skip-cert-verify"].as_bool(), Some(true));
    }

    #[test]
    fn preserves_tuic_allow_insecure_alias() {
        let url = Url::parse(
            "tuic://00000000-0000-0000-0000-000000000001:secret@tuic.example:443?allow_insecure=1",
        )
        .expect("URL");
        let (proxy, _, _, _) = parse_tuic(url, None).expect("proxy");
        assert_eq!(proxy["skip-cert-verify"].as_bool(), Some(true));
    }
}

fn insert_number(map: &mut Mapping, key: &str, value: u64) {
    map.insert(
        Value::String(key.to_owned()),
        Value::Number(serde_yaml::Number::from(value)),
    );
}

pub(super) fn safe_name(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() || value.chars().any(char::is_control) || value.chars().count() > 256 {
        return None;
    }
    Some(value.to_owned())
}
