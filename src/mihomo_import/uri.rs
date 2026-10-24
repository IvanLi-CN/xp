use super::{Candidate, meaningful_lines};
use base64::Engine as _;
use reqwest::Url;
use serde_yaml::{Mapping, Value};
use std::collections::BTreeMap;

pub(super) fn parse_raw_candidates(source: &str) -> Vec<Candidate> {
    meaningful_lines(source)
        .into_iter()
        .map(|line| {
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
        })
        .collect()
}

fn parse_uri_proxy(source: &str) -> Result<(Value, Option<String>, String, Vec<String>), String> {
    let url = Url::parse(source).map_err(|_| "URI is malformed".to_owned())?;
    let scheme = url.scheme().to_ascii_lowercase();
    let fragment_name = url
        .fragment()
        .and_then(decode_component)
        .and_then(|name| safe_name(&name));
    match scheme.as_str() {
        "vless" => parse_vless(url, fragment_name),
        "ss" => parse_ss(url, fragment_name),
        "vmess" => parse_vmess(url, fragment_name),
        "trojan" => parse_trojan(url, fragment_name),
        "hysteria2" | "hy2" => parse_hysteria2(url, fragment_name),
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
            .filter(|value| !value.is_empty())
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
) -> Result<(Value, Option<String>, String, Vec<String>), String> {
    let (method, password, server, port) = parse_ss_parts(&url)?;
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

fn parse_ss_parts(url: &Url) -> Result<(String, String, String, u16), String> {
    let direct_user = decode_component(url.username()).unwrap_or_default();
    let direct_password = url.password().and_then(decode_component);
    if let (Some(server), Some(port), Some(password)) = (
        url.host_str().filter(|value| !value.is_empty()),
        url.port(),
        direct_password,
    ) {
        return Ok((direct_user, password, server.to_owned(), port));
    }

    if let (Some(server), Some(port)) =
        (url.host_str().filter(|value| !value.is_empty()), url.port())
        && let Some(decoded) = decode_base64_bytes(&direct_user)
        && let Ok(decoded) = String::from_utf8(decoded)
        && let Some((method, password)) = decoded.split_once(':')
    {
        return Ok((
            method.to_owned(),
            password.to_owned(),
            server.to_owned(),
            port,
        ));
    }

    let encoded = if !direct_user.is_empty() && url.host_str().is_some() {
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
    let authority_url = Url::parse(&format!("http://{authority}"))
        .map_err(|_| "ss payload server is invalid".to_owned())?;
    let server = authority_url
        .host_str()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "ss payload server is required".to_owned())?;
    let port = authority_url
        .port()
        .ok_or_else(|| "ss payload port is required".to_owned())?;
    Ok((
        method.to_owned(),
        password.to_owned(),
        server.to_owned(),
        port,
    ))
}

fn parse_vmess(
    url: Url,
    fragment_name: Option<String>,
) -> Result<(Value, Option<String>, String, Vec<String>), String> {
    let encoded = if !url.path().trim_matches('/').is_empty() {
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

    let mut map = base_proxy("vmess", &server, port);
    insert_string(&mut map, "uuid", uuid);
    insert_string(&mut map, "network", network.as_str());
    insert_string(
        &mut map,
        "cipher",
        json_string(&payload, "scy").unwrap_or_else(|| "auto".to_owned()),
    );
    insert_bool(&mut map, "udp", true);
    let tls = json_string(&payload, "tls")
        .map(|value| value == "tls" || value == "true")
        .or_else(|| payload.get("tls").and_then(serde_json::Value::as_bool))
        .unwrap_or(false);
    insert_bool(&mut map, "tls", tls);
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
    }
    let name =
        fragment_name.or_else(|| json_string(&payload, "ps").and_then(|value| safe_name(&value)));
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
) -> Result<(Value, Option<String>, String, Vec<String>), String> {
    let password = decode_component(url.username())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "hysteria2 password is required".to_owned())?;
    let (server, port) = host_port(&url, "hysteria2")?;
    let query = query_map(&url);
    let mut map = base_proxy("hysteria2", &server, port);
    insert_string(&mut map, "password", password);
    if let Some(sni) = query.get("sni") {
        insert_string(&mut map, "sni", sni.clone());
    }
    if let Some(insecure) = query_bool(&query, "insecure") {
        insert_bool(&mut map, "skip-cert-verify", insecure);
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
    if let Some(value) = query_bool(&query, "insecure") {
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
    let server = url
        .host_str()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("{scheme} server is required"))?;
    let port = url
        .port()
        .ok_or_else(|| format!("{scheme} port is required"))?;
    Ok((server.to_owned(), port))
}

fn supported_network(network: &str) -> bool {
    matches!(network, "tcp" | "ws" | "grpc" | "http" | "h2")
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
            .or_else(|| value.as_str()?.parse().ok())
    })
}

pub(super) fn safe_name(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() || value.chars().any(char::is_control) {
        return None;
    }
    let mut name = value.chars().take(80).collect::<String>();
    if name.is_empty() {
        return None;
    }
    if name.len() < value.len() {
        name.push_str("...");
    }
    Some(name)
}
