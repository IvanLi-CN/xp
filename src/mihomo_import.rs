use base64::Engine as _;
use reqwest::Url;
use serde::Serialize;
use serde_yaml::{Mapping, Value};
use std::collections::{BTreeMap, BTreeSet};

use crate::mihomo_redact::{self, RedactErrorKind, UrlLoadPolicy};

mod uri;

pub const MAX_SOURCE_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_CANDIDATES: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DetectedSource {
    RemoteUrl,
    Raw,
    Base64,
    MihomoYaml,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ImportItemStatus {
    Added,
    Skipped,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ImportItem {
    pub source_index: usize,
    pub status: ImportItemStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scheme: Option<String>,
    pub warnings: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ImportCounts {
    pub added: usize,
    pub skipped: usize,
    pub failed: usize,
    pub warnings: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ImportPreview {
    pub detected_source: DetectedSource,
    pub added_proxies_yaml: String,
    pub counts: ImportCounts,
    pub items: Vec<ImportItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportError {
    Invalid(String),
    TooLarge(String),
    Upstream(String),
}

impl ImportError {
    fn invalid(message: impl Into<String>) -> Self {
        Self::Invalid(message.into())
    }

    fn too_large(message: impl Into<String>) -> Self {
        Self::TooLarge(message.into())
    }

    fn upstream(message: impl Into<String>) -> Self {
        Self::Upstream(message.into())
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Candidate {
    pub(crate) scheme: Option<String>,
    pub(crate) name: Option<String>,
    pub(crate) proxy: Option<Value>,
    pub(crate) warnings: Vec<String>,
    pub(crate) failure: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ContentKind {
    Raw,
    Base64,
    MihomoYaml,
}

pub async fn preview(
    source: &str,
    existing_extra_proxies_yaml: &str,
    reserved_names: &BTreeSet<String>,
) -> Result<ImportPreview, ImportError> {
    if source.len() > MAX_SOURCE_BYTES {
        return Err(ImportError::too_large(format!(
            "source exceeds {MAX_SOURCE_BYTES} bytes"
        )));
    }
    if source.trim().is_empty() {
        return Err(ImportError::invalid("source is empty"));
    }

    let (detected_source, payload) = if let Some(url) = single_http_url(source)? {
        let loaded = mihomo_redact::load_text_from_url(&url, 15, UrlLoadPolicy::PublicOnly)
            .await
            .map_err(map_remote_error)?;
        (DetectedSource::RemoteUrl, loaded)
    } else {
        (ContentKind::detect(source)?, source.to_owned()).into_detected_source_and_payload()
    };

    let existing = parse_existing_extra_proxies(existing_extra_proxies_yaml)?;
    let candidates = match detected_source {
        DetectedSource::RemoteUrl => parse_content(&payload)?.1,
        DetectedSource::Raw => uri::parse_raw_candidates(&payload),
        DetectedSource::Base64 => {
            let decoded = decode_base64_text(&payload)?;
            parse_decoded_content(&decoded)?.1
        }
        DetectedSource::MihomoYaml => parse_yaml_candidates(&payload)?,
    };
    build_preview(detected_source, existing, candidates, reserved_names)
}

pub fn preview_text(
    source: &str,
    existing_extra_proxies_yaml: &str,
    reserved_names: &BTreeSet<String>,
) -> Result<ImportPreview, ImportError> {
    if source.len() > MAX_SOURCE_BYTES {
        return Err(ImportError::too_large(format!(
            "source exceeds {MAX_SOURCE_BYTES} bytes"
        )));
    }
    if source.trim().is_empty() {
        return Err(ImportError::invalid("source is empty"));
    }

    let (kind, candidates) = parse_content(source)?;
    let detected = match kind {
        ContentKind::Raw => DetectedSource::Raw,
        ContentKind::Base64 => DetectedSource::Base64,
        ContentKind::MihomoYaml => DetectedSource::MihomoYaml,
    };
    let existing = parse_existing_extra_proxies(existing_extra_proxies_yaml)?;
    build_preview(detected, existing, candidates, reserved_names)
}

fn map_remote_error(error: mihomo_redact::RedactError) -> ImportError {
    if error.message.contains("exceeds") {
        return ImportError::too_large("fetched source exceeds the size limit");
    }
    if error.message.contains("source body is not valid utf-8") {
        return ImportError::upstream("remote source returned invalid text");
    }
    match error.kind {
        RedactErrorKind::InvalidInput => ImportError::invalid("remote source was rejected"),
        RedactErrorKind::Network => ImportError::upstream("remote source could not be fetched"),
    }
}

fn single_http_url(source: &str) -> Result<Option<String>, ImportError> {
    let lines = meaningful_lines(source);
    if lines.len() != 1 {
        return Ok(None);
    }

    let line = lines[0].trim();
    let has_http_prefix = line.starts_with("http://") || line.starts_with("https://");
    let Ok(url) = Url::parse(line) else {
        return if has_http_prefix {
            Err(ImportError::invalid("source URL is invalid"))
        } else {
            Ok(None)
        };
    };
    if matches!(url.scheme(), "http" | "https") {
        return Ok(Some(line.to_owned()));
    }
    Ok(None)
}

fn meaningful_lines(source: &str) -> Vec<&str> {
    source
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect()
}

impl ContentKind {
    fn detect(source: &str) -> Result<Self, ImportError> {
        if source_looks_like_yaml(source) {
            parse_yaml_candidates(source)?;
            return Ok(Self::MihomoYaml);
        }

        if let Ok(Value::Mapping(map)) = serde_yaml::from_str::<Value>(source) {
            if map.contains_key(Value::String("proxies".to_owned())) {
                parse_yaml_candidates(source)?;
                return Ok(Self::MihomoYaml);
            }
            return Err(ImportError::invalid(
                "YAML source must contain a root proxies sequence",
            ));
        }

        if looks_like_base64(source) {
            let decoded = decode_base64_text(source)?;
            if decoded.contains("://") || source_looks_like_yaml(&decoded) {
                let _ = parse_decoded_content(&decoded)?;
                return Ok(Self::Base64);
            }
            return Err(ImportError::invalid(
                "base64 source does not contain supported subscription text",
            ));
        }

        if meaningful_lines(source)
            .iter()
            .any(|line| line.contains("://"))
        {
            return Ok(Self::Raw);
        }

        Err(ImportError::invalid(
            "source is not a supported URL, Base64 subscription, URI list, or Mihomo YAML",
        ))
    }
}

trait ContentKindExt {
    fn into_detected_source_and_payload(self) -> (DetectedSource, String);
}

impl ContentKindExt for (ContentKind, String) {
    fn into_detected_source_and_payload(self) -> (DetectedSource, String) {
        let (kind, payload) = self;
        let detected = match kind {
            ContentKind::Raw => DetectedSource::Raw,
            ContentKind::Base64 => DetectedSource::Base64,
            ContentKind::MihomoYaml => DetectedSource::MihomoYaml,
        };
        (detected, payload)
    }
}

fn parse_content(source: &str) -> Result<(ContentKind, Vec<Candidate>), ImportError> {
    let kind = ContentKind::detect(source)?;
    match kind {
        ContentKind::Raw => Ok((kind, uri::parse_raw_candidates(source))),
        ContentKind::Base64 => {
            let decoded = decode_base64_text(source)?;
            let (_, candidates) = parse_decoded_content(&decoded)?;
            Ok((kind, candidates))
        }
        ContentKind::MihomoYaml => Ok((kind, parse_yaml_candidates(source)?)),
    }
}

fn parse_decoded_content(source: &str) -> Result<(ContentKind, Vec<Candidate>), ImportError> {
    if source_looks_like_yaml(source) {
        return Ok((ContentKind::MihomoYaml, parse_yaml_candidates(source)?));
    }
    Ok((ContentKind::Raw, uri::parse_raw_candidates(source)))
}

fn source_looks_like_yaml(source: &str) -> bool {
    let trimmed = source.trim_start();
    trimmed.starts_with('{')
        || trimmed.starts_with("proxies:")
        || trimmed.starts_with("proxy-groups:")
        || trimmed.starts_with("proxy-providers:")
        || trimmed.starts_with("mixed-port:")
        || trimmed.starts_with("port:")
        || trimmed.lines().any(|line| {
            let line = line.trim_start();
            line.starts_with("proxies:")
                || line.starts_with("proxy-groups:")
                || line.starts_with("proxy-providers:")
        })
}

fn looks_like_base64(source: &str) -> bool {
    let compact: String = source
        .chars()
        .filter(|ch| !ch.is_ascii_whitespace())
        .collect();
    compact.len() >= 16
        && compact
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '/' | '=' | '-' | '_'))
}

fn decode_base64_text(source: &str) -> Result<String, ImportError> {
    let compact: String = source
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
            if let Ok(bytes) = engine.decode(&candidate)
                && let Ok(text) = String::from_utf8(bytes)
            {
                return Ok(text);
            }
        }
    }
    Err(ImportError::invalid("base64 source could not be decoded"))
}

fn parse_yaml_candidates(source: &str) -> Result<Vec<Candidate>, ImportError> {
    let root = serde_yaml::from_str::<Value>(source)
        .map_err(|_| ImportError::invalid("source YAML is malformed"))?;
    let Value::Mapping(map) = root else {
        return Err(ImportError::invalid(
            "Mihomo YAML root must be a mapping with proxies",
        ));
    };
    let Some(proxies) = map.get(Value::String("proxies".to_owned())) else {
        return Err(ImportError::invalid(
            "Mihomo YAML must contain a root proxies sequence",
        ));
    };
    let Value::Sequence(proxies) = proxies else {
        return Err(ImportError::invalid(
            "Mihomo YAML proxies must be a sequence",
        ));
    };
    ensure_candidate_bound(proxies.len())?;

    let mut ignored = map
        .keys()
        .filter_map(Value::as_str)
        .filter(|key| *key != "proxies")
        .map(str::to_owned)
        .collect::<Vec<_>>();
    ignored.sort();
    let root_warning =
        (!ignored.is_empty()).then(|| format!("ignored_root_fields:{}", ignored.join(",")));

    Ok(proxies
        .iter()
        .map(|proxy| {
            let mut candidate = yaml_candidate(proxy);
            if let Some(warning) = &root_warning {
                candidate.warnings.push(warning.clone());
            }
            candidate
        })
        .collect())
}

fn yaml_candidate(proxy: &Value) -> Candidate {
    let Value::Mapping(map) = proxy else {
        return Candidate {
            scheme: None,
            name: None,
            proxy: None,
            warnings: Vec::new(),
            failure: Some("proxy item must be a mapping".to_owned()),
        };
    };

    let name = match map.get(Value::String("name".to_owned())) {
        None => None,
        Some(Value::String(name)) => uri::safe_name(name),
        Some(_) => {
            return Candidate {
                scheme: yaml_scheme(map),
                name: None,
                proxy: None,
                warnings: Vec::new(),
                failure: Some("proxy name must be a string".to_owned()),
            };
        }
    };
    let Some(Value::String(scheme)) = map.get(Value::String("type".to_owned())) else {
        return Candidate {
            scheme: yaml_scheme(map),
            name,
            proxy: None,
            warnings: Vec::new(),
            failure: Some("proxy type is required".to_owned()),
        };
    };
    if scheme.trim().is_empty() {
        return Candidate {
            scheme: None,
            name,
            proxy: None,
            warnings: Vec::new(),
            failure: Some("proxy type is required".to_owned()),
        };
    }
    let scheme = scheme.trim().to_ascii_lowercase();
    if let Some(reason) = validate_yaml_proxy(map, &scheme) {
        return Candidate {
            scheme: Some(scheme),
            name,
            proxy: None,
            warnings: Vec::new(),
            failure: Some(reason),
        };
    }

    Candidate {
        scheme: Some(scheme),
        name,
        proxy: Some(Value::Mapping(map.clone())),
        warnings: Vec::new(),
        failure: None,
    }
}

fn yaml_scheme(map: &Mapping) -> Option<String> {
    map.get(Value::String("type".to_owned()))
        .and_then(Value::as_str)
        .map(|value| value.trim().to_ascii_lowercase())
}

fn validate_yaml_proxy(map: &Mapping, scheme: &str) -> Option<String> {
    let known = matches!(
        scheme,
        "vless" | "ss" | "vmess" | "trojan" | "hysteria2" | "hy2" | "tuic"
    );
    if !known {
        return None;
    }
    if !yaml_nonempty_string(map, "server") {
        return Some(format!("{scheme} server is required"));
    }
    if !yaml_port(map) {
        return Some(format!("{scheme} port is required"));
    }
    let required: &[&str] = match scheme {
        "vless" | "vmess" | "tuic" => &["uuid"],
        "ss" => &["cipher", "password"],
        "trojan" | "hysteria2" | "hy2" => &["password"],
        _ => &[],
    };
    for field in required {
        if !yaml_nonempty_string(map, field) {
            return Some(format!("{scheme} {field} is required"));
        }
    }
    if scheme == "tuic" && !yaml_nonempty_string(map, "password") {
        return Some("tuic password is required".to_owned());
    }
    None
}

fn yaml_nonempty_string(map: &Mapping, key: &str) -> bool {
    matches!(
        map.get(Value::String(key.to_owned())),
        Some(Value::String(value)) if !value.trim().is_empty()
    )
}

fn yaml_port(map: &Mapping) -> bool {
    match map.get(Value::String("port".to_owned())) {
        Some(Value::Number(value)) => value
            .as_u64()
            .is_some_and(|port| port > 0 && port <= u16::MAX as u64),
        Some(Value::String(value)) => value.parse::<u16>().is_ok_and(|port| port > 0),
        _ => false,
    }
}

fn parse_existing_extra_proxies(input: &str) -> Result<Vec<Value>, ImportError> {
    if input.trim().is_empty() {
        return Ok(Vec::new());
    }
    let value = serde_yaml::from_str::<Value>(input)
        .map_err(|_| ImportError::invalid("existing_extra_proxies_yaml is malformed"))?;
    let Value::Sequence(proxies) = value else {
        return Err(ImportError::invalid(
            "existing_extra_proxies_yaml must be a YAML sequence or empty",
        ));
    };
    for proxy in &proxies {
        let Value::Mapping(map) = proxy else {
            return Err(ImportError::invalid(
                "existing_extra_proxies_yaml contains a non-mapping proxy",
            ));
        };
        if !matches!(
            map.get(Value::String("name".to_owned())),
            Some(Value::String(name)) if !name.trim().is_empty()
        ) {
            return Err(ImportError::invalid(
                "existing_extra_proxies_yaml contains a proxy without a name",
            ));
        }
        if !matches!(
            map.get(Value::String("type".to_owned())),
            Some(Value::String(kind)) if !kind.trim().is_empty()
        ) {
            return Err(ImportError::invalid(
                "existing_extra_proxies_yaml contains a proxy without a type",
            ));
        }
    }
    Ok(proxies)
}

fn ensure_candidate_bound(count: usize) -> Result<(), ImportError> {
    if count > MAX_CANDIDATES {
        return Err(ImportError::too_large(format!(
            "source contains more than {MAX_CANDIDATES} candidates"
        )));
    }
    Ok(())
}

fn build_preview(
    detected_source: DetectedSource,
    existing: Vec<Value>,
    candidates: Vec<Candidate>,
    reserved_names: &BTreeSet<String>,
) -> Result<ImportPreview, ImportError> {
    ensure_candidate_bound(candidates.len())?;
    if candidates.is_empty() {
        return Err(ImportError::invalid("source contains no candidate nodes"));
    }

    let mut used_names = reserved_names.clone();
    let mut existing_fingerprints = BTreeSet::new();
    for proxy in &existing {
        if let Value::Mapping(map) = proxy
            && let Some(Value::String(name)) = map.get(Value::String("name".to_owned()))
        {
            used_names.insert(name.clone());
        }
        existing_fingerprints.insert(canonical_proxy(proxy));
    }
    let mut seen_fingerprints = existing_fingerprints.clone();
    let mut accepted = Vec::new();
    let mut items = Vec::with_capacity(candidates.len());
    let mut counts = ImportCounts {
        added: 0,
        skipped: 0,
        failed: 0,
        warnings: 0,
    };

    for (index, candidate) in candidates.into_iter().enumerate() {
        let source_index = index + 1;
        let mut warnings = candidate.warnings;
        counts.warnings += warnings.len();
        if let Some(reason) = candidate.failure {
            counts.failed += 1;
            items.push(ImportItem {
                source_index,
                status: ImportItemStatus::Failed,
                name: candidate.name,
                scheme: candidate.scheme,
                warnings,
                reason: Some(reason),
            });
            continue;
        }
        let Some(mut proxy) = candidate.proxy else {
            counts.failed += 1;
            items.push(ImportItem {
                source_index,
                status: ImportItemStatus::Failed,
                name: candidate.name,
                scheme: candidate.scheme,
                warnings,
                reason: Some("proxy could not be converted".to_owned()),
            });
            continue;
        };

        let fingerprint = canonical_proxy(&proxy);
        if seen_fingerprints.contains(&fingerprint) {
            counts.skipped += 1;
            items.push(ImportItem {
                source_index,
                status: ImportItemStatus::Skipped,
                name: candidate.name,
                scheme: candidate.scheme,
                warnings,
                reason: Some("duplicate".to_owned()),
            });
            continue;
        }

        let base_name = candidate
            .name
            .or_else(|| Some(format!("imported-{source_index:03}")))
            .unwrap_or_default();
        if reserved_names.contains(&base_name) {
            counts.failed += 1;
            items.push(ImportItem {
                source_index,
                status: ImportItemStatus::Failed,
                name: Some(base_name),
                scheme: candidate.scheme,
                warnings,
                reason: Some("reserved name".to_owned()),
            });
            continue;
        }
        let final_name = allocate_name(&base_name, &used_names);
        if final_name != base_name {
            warnings.push("name_collision_suffix_allocated".to_owned());
            counts.warnings += 1;
        }
        proxy = assign_name(proxy, &final_name);
        seen_fingerprints.insert(fingerprint);
        used_names.insert(final_name.clone());
        accepted.push(proxy);
        counts.added += 1;
        items.push(ImportItem {
            source_index,
            status: ImportItemStatus::Added,
            name: Some(final_name),
            scheme: candidate.scheme,
            warnings,
            reason: None,
        });
    }

    let added_proxies_yaml = if accepted.is_empty() {
        String::new()
    } else {
        serde_yaml::to_string(&Value::Sequence(accepted))
            .map_err(|_| ImportError::invalid("accepted proxies could not be serialized"))?
    };
    Ok(ImportPreview {
        detected_source,
        added_proxies_yaml,
        counts,
        items,
    })
}

fn allocate_name(base: &str, used_names: &BTreeSet<String>) -> String {
    if !used_names.contains(base) {
        return base.to_owned();
    }
    for suffix in 2..=u32::MAX {
        let candidate = format!("{base}-{suffix}");
        if !used_names.contains(&candidate) {
            return candidate;
        }
    }
    unreachable!("name suffix space is exhausted")
}

fn assign_name(value: Value, name: &str) -> Value {
    let Value::Mapping(map) = value else {
        return value;
    };
    let mut output = Mapping::new();
    uri::insert_string(&mut output, "name", name);
    for (key, value) in map {
        if key != Value::String("name".to_owned()) {
            output.insert(key, value);
        }
    }
    Value::Mapping(output)
}

fn canonical_proxy(value: &Value) -> String {
    let canonical = canonical_value(value, true);
    serde_json::to_string(&canonical).unwrap_or_default()
}

fn canonical_value(value: &Value, ignore_name: bool) -> serde_json::Value {
    match value {
        Value::Null => serde_json::Value::Null,
        Value::Bool(value) => serde_json::Value::Bool(*value),
        Value::Number(value) => serde_json::to_value(value).unwrap_or(serde_json::Value::Null),
        Value::String(value) => serde_json::Value::String(value.clone()),
        Value::Sequence(values) => serde_json::Value::Array(
            values
                .iter()
                .map(|value| canonical_value(value, false))
                .collect(),
        ),
        Value::Mapping(map) => {
            let mut entries = BTreeMap::new();
            for (key, value) in map {
                let Some(key) = key.as_str() else {
                    continue;
                };
                if ignore_name && key == "name" {
                    continue;
                }
                entries.insert(key.to_owned(), canonical_value(value, false));
            }
            serde_json::Value::Object(entries.into_iter().collect())
        }
        Value::Tagged(tagged) => canonical_value(&tagged.value, ignore_name),
    }
}

#[cfg(test)]
mod tests {
    use super::{DetectedSource, ImportItemStatus, preview_text};
    use base64::Engine as _;
    use std::collections::BTreeSet;

    fn empty_reserved() -> BTreeSet<String> {
        BTreeSet::new()
    }

    #[test]
    fn imports_all_supported_uri_schemes() {
        let vmess_payload = br#"{
  "v": "2",
  "ps": "vmess-node",
  "add": "vmess.example",
  "port": "443",
  "id": "00000000-0000-0000-0000-000000000001",
  "net": "ws",
  "tls": "tls",
  "path": "/ws",
  "host": "vmess.example"
}"#;
        let vmess = base64::engine::general_purpose::STANDARD.encode(vmess_payload);
        let source = format!(
            concat!(
                "vless://00000000-0000-0000-0000-000000000002@vless.example:443?",
                "security=tls&sni=vless.example#vless\n",
                "vmess://{vmess}\n",
                "ss://YWVzLTI1Ni1nY206cGFzc3dvcmQ@example.com:443#ss\n",
                "trojan://secret@trojan.example:443?sni=trojan.example#trojan\n",
                "hysteria2://secret@hy.example:443?sni=hy.example#hy\n",
                "tuic://00000000-0000-0000-0000-000000000003:secret@tuic.example:443?",
                "sni=tuic.example#tuic"
            ),
            vmess = vmess,
        );
        let preview = preview_text(&source, "", &empty_reserved()).expect("preview");
        assert_eq!(preview.detected_source, DetectedSource::Raw);
        assert_eq!(preview.counts.added, 6);
        assert!(
            preview
                .items
                .iter()
                .all(|item| item.status == ImportItemStatus::Added)
        );
    }

    #[test]
    fn imports_url_safe_unpadded_base64_and_yaml() {
        let yaml = r#"mixed-port: 7890
proxy-groups:
  - name: ignored
proxies:
  - type: ss
    server: example.com
    port: 443
    cipher: aes-256-gcm
    password: secret
"#;
        let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(yaml);
        let preview = preview_text(&encoded, "", &empty_reserved()).expect("preview");
        assert_eq!(preview.detected_source, DetectedSource::Base64);
        assert_eq!(preview.counts.added, 1);
        assert_eq!(preview.counts.warnings, 1);
        assert!(
            preview.items[0]
                .warnings
                .iter()
                .any(|warning| warning.starts_with("ignored_root_fields:"))
        );
    }

    #[test]
    fn skips_duplicates_and_allocates_names_without_rewriting_existing_text() {
        let existing = r#"# keep this comment
- name: old
  type: ss
  server: example.com
  port: 443
  cipher: aes-256-gcm
  password: secret
  udp: true
"#;
        let source = r#"ss://YWVzLTI1Ni1nY206c2VjcmV0@example.com:443#old
ss://YWVzLTI1Ni1nY206cGFzc3dvcmQ@example.com:443#old
"#;
        let preview = preview_text(source, existing, &empty_reserved()).expect("preview");
        assert_eq!(preview.counts.skipped, 1);
        assert_eq!(preview.counts.added, 1);
        assert!(preview.added_proxies_yaml.contains("old-2"));
        assert!(!preview.added_proxies_yaml.contains("- name: old\n"));
    }

    #[test]
    fn rejects_reserved_names_and_unsupported_schemes_as_items() {
        let reserved = BTreeSet::from(["blocked".to_owned()]);
        let preview = preview_text(
            r#"ss://YWVzLTI1Ni1nY206cGFzc3dvcmQ@example.com:443#blocked
ssr://secret@example.com:443#unsupported"#,
            "",
            &reserved,
        )
        .expect("preview");
        assert_eq!(preview.counts.failed, 2);
        assert!(preview.added_proxies_yaml.is_empty());
        assert!(
            preview
                .items
                .iter()
                .all(|item| item.status == ImportItemStatus::Failed)
        );
    }

    #[test]
    fn validates_known_yaml_fields_but_preserves_unknown_types() {
        let preview = preview_text(
            r#"proxies:
  - name: incomplete
    type: ss
    password: secret
  - name: custom
    type: vendor-private
    server: example.com
    port: 443
    custom-field: keep
"#,
            "",
            &empty_reserved(),
        )
        .expect("preview");
        assert_eq!(preview.counts.added, 1);
        assert_eq!(preview.counts.failed, 1);
        assert!(
            preview.items[0]
                .reason
                .as_deref()
                .unwrap()
                .contains("server")
        );
        assert!(preview.added_proxies_yaml.contains("vendor-private"));
        assert!(preview.added_proxies_yaml.contains("custom-field"));
    }

    #[test]
    fn rejects_invalid_existing_draft() {
        let error = preview_text("ssr://unsupported", "not-a-sequence", &empty_reserved())
            .expect_err("invalid draft");
        assert!(matches!(error, super::ImportError::Invalid(_)));
    }
}
