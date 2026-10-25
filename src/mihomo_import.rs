use base64::Engine as _;
use reqwest::Url;
use serde::Serialize;
use serde_yaml::{Mapping, Value};
use std::collections::BTreeSet;

use crate::mihomo_redact::{self, RedactErrorKind, UrlLoadPolicy};

#[path = "mihomo_import_names.rs"]
mod import_names;
mod uri;

pub const MAX_SOURCE_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_CANDIDATES: usize = 512;
const MAX_YAML_NESTING_DEPTH: usize = 128;

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
        DetectedSource::Raw => uri::parse_raw_candidates(&payload)?,
        DetectedSource::Base64 => {
            let decoded = decode_base64_text(&payload)?;
            parse_decoded_content(&decoded)?.1
        }
        DetectedSource::MihomoYaml => parse_yaml_candidates(&payload)?,
    };
    let preview = build_preview(detected_source, existing, candidates, reserved_names)?;
    ensure_appendable_extra_proxies_yaml(
        existing_extra_proxies_yaml,
        preview.counts.added,
        &preview.added_proxies_yaml,
    )?;
    Ok(preview)
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
    let preview = build_preview(detected, existing, candidates, reserved_names)?;
    ensure_appendable_extra_proxies_yaml(
        existing_extra_proxies_yaml,
        preview.counts.added,
        &preview.added_proxies_yaml,
    )?;
    Ok(preview)
}

fn map_remote_error(error: mihomo_redact::RedactError) -> ImportError {
    if error.message.contains("source body is not valid utf-8") {
        return ImportError::upstream("remote source returned invalid text");
    }
    match error.kind {
        RedactErrorKind::InvalidInput => ImportError::invalid("remote source was rejected"),
        RedactErrorKind::TooLarge => {
            ImportError::too_large("fetched source exceeds the size limit")
        }
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
        .split(['\n', '\r'])
        .map(str::trim)
        .map(|line| line.strip_prefix('\u{feff}').unwrap_or(line))
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect()
}

impl ContentKind {
    fn detect(source: &str) -> Result<Self, ImportError> {
        reject_yaml_aliases(source)?;
        if try_parse_mihomo_yaml(source).is_some() {
            return Ok(Self::MihomoYaml);
        }
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
            reject_yaml_aliases(&decoded)?;
            if decoded.contains("://")
                || source_looks_like_yaml(&decoded)
                || try_parse_mihomo_yaml(&decoded).is_some()
            {
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
        ContentKind::Raw => Ok((kind, uri::parse_raw_candidates(source)?)),
        ContentKind::Base64 => {
            let decoded = decode_base64_text(source)?;
            let (_, candidates) = parse_decoded_content(&decoded)?;
            Ok((kind, candidates))
        }
        ContentKind::MihomoYaml => Ok((kind, parse_yaml_candidates(source)?)),
    }
}

fn parse_decoded_content(source: &str) -> Result<(ContentKind, Vec<Candidate>), ImportError> {
    reject_yaml_aliases(source)?;
    if try_parse_mihomo_yaml(source).is_some() {
        return Ok((ContentKind::MihomoYaml, parse_yaml_candidates(source)?));
    }
    if source_looks_like_yaml(source) {
        return Ok((ContentKind::MihomoYaml, parse_yaml_candidates(source)?));
    }
    Ok((ContentKind::Raw, uri::parse_raw_candidates(source)?))
}

fn try_parse_mihomo_yaml(source: &str) -> Option<()> {
    let Value::Mapping(map) = serde_yaml::from_str::<Value>(source).ok()? else {
        return None;
    };
    map.contains_key(Value::String("proxies".to_owned()))
        .then_some(())
}

fn reject_yaml_aliases(source: &str) -> Result<(), ImportError> {
    if source
        .chars()
        .any(|character| matches!(character, '\u{0085}' | '\u{2028}' | '\u{2029}'))
    {
        return Err(ImportError::invalid("unsupported YAML line ending"));
    }

    let lines = meaningful_lines(source);
    if lines.is_empty() || lines.iter().all(|line| is_uri_line(line)) {
        return Ok(());
    }

    let has_reference_marker = source
        .chars()
        .any(|character| matches!(character, '&' | '*'));
    if !source_is_yaml_candidate(source) && !has_reference_marker {
        return Ok(());
    }

    if yaml_lex_exceeds_nesting(source) {
        return Err(ImportError::invalid("source YAML nesting exceeds limit"));
    }

    let lexical_anchor_or_alias = yaml_lex_contains_structural_anchor_or_alias(source)
        || yaml_edit::lex(source).iter().any(|(kind, text)| {
            *kind == yaml_edit::SyntaxKind::UNTERMINATED_STRING
                && yaml_lex_contains_structural_anchor_or_alias(
                    text.strip_prefix('"').unwrap_or(text),
                )
        });
    if lexical_anchor_or_alias {
        return Err(ImportError::invalid(
            "YAML anchors and aliases are unsupported",
        ));
    }
    Ok(())
}

fn yaml_node_start_is_structural(
    previous_significant: Option<yaml_edit::SyntaxKind>,
    previous_string_is_quoted: bool,
) -> bool {
    match previous_significant {
        None
        | Some(yaml_edit::SyntaxKind::COLON)
        | Some(yaml_edit::SyntaxKind::DASH)
        | Some(yaml_edit::SyntaxKind::LEFT_BRACKET)
        | Some(yaml_edit::SyntaxKind::LEFT_BRACE)
        | Some(yaml_edit::SyntaxKind::COMMA)
        | Some(yaml_edit::SyntaxKind::DOC_START)
        | Some(yaml_edit::SyntaxKind::TAG)
        | Some(yaml_edit::SyntaxKind::ANCHOR)
        | Some(yaml_edit::SyntaxKind::QUESTION)
        | Some(yaml_edit::SyntaxKind::REFERENCE)
        | Some(yaml_edit::SyntaxKind::RIGHT_BRACKET)
        | Some(yaml_edit::SyntaxKind::RIGHT_BRACE) => true,
        Some(yaml_edit::SyntaxKind::STRING) => previous_string_is_quoted,
        _ => false,
    }
}

fn yaml_property_token_is_structural(
    kind: yaml_edit::SyntaxKind,
    previous_significant: Option<yaml_edit::SyntaxKind>,
    previous_string_is_quoted: bool,
) -> bool {
    matches!(
        kind,
        yaml_edit::SyntaxKind::TAG
            | yaml_edit::SyntaxKind::ANCHOR
            | yaml_edit::SyntaxKind::REFERENCE
    ) && yaml_node_start_is_structural(previous_significant, previous_string_is_quoted)
}

fn yaml_block_scalar_indicator_is_structural(
    kind: yaml_edit::SyntaxKind,
    previous_significant: Option<yaml_edit::SyntaxKind>,
    previous_string_is_quoted: bool,
) -> bool {
    matches!(
        kind,
        yaml_edit::SyntaxKind::PIPE | yaml_edit::SyntaxKind::GREATER
    ) && yaml_node_start_is_structural(previous_significant, previous_string_is_quoted)
}

fn yaml_explicit_block_scalar_indent(text: &str) -> Option<usize> {
    let mut indent = None;
    for character in text.chars() {
        match character {
            '1'..='9' if indent.is_none() => indent = Some(character as usize - '0' as usize),
            '+' | '-' => {}
            _ => return None,
        }
    }
    indent
}

fn yaml_flow_token_is_structural(
    kind: yaml_edit::SyntaxKind,
    flow_depth: usize,
    previous_significant: Option<yaml_edit::SyntaxKind>,
    previous_string_is_quoted: bool,
) -> bool {
    match kind {
        yaml_edit::SyntaxKind::LEFT_BRACKET | yaml_edit::SyntaxKind::LEFT_BRACE => {
            flow_depth > 0
                || yaml_node_start_is_structural(previous_significant, previous_string_is_quoted)
        }
        yaml_edit::SyntaxKind::RIGHT_BRACKET | yaml_edit::SyntaxKind::RIGHT_BRACE => flow_depth > 0,
        _ => false,
    }
}

fn yaml_previous_significant_after_newline(
    previous_significant: Option<yaml_edit::SyntaxKind>,
    previous_string_is_quoted: bool,
) -> (Option<yaml_edit::SyntaxKind>, bool) {
    let previous_significant = match previous_significant {
        Some(
            yaml_edit::SyntaxKind::COLON
            | yaml_edit::SyntaxKind::DASH
            | yaml_edit::SyntaxKind::LEFT_BRACKET
            | yaml_edit::SyntaxKind::LEFT_BRACE
            | yaml_edit::SyntaxKind::COMMA
            | yaml_edit::SyntaxKind::DOC_START
            | yaml_edit::SyntaxKind::TAG
            | yaml_edit::SyntaxKind::QUESTION
            | yaml_edit::SyntaxKind::STRING
            | yaml_edit::SyntaxKind::INT
            | yaml_edit::SyntaxKind::FLOAT
            | yaml_edit::SyntaxKind::BOOL
            | yaml_edit::SyntaxKind::NULL
            | yaml_edit::SyntaxKind::ANCHOR
            | yaml_edit::SyntaxKind::REFERENCE
            | yaml_edit::SyntaxKind::RIGHT_BRACKET
            | yaml_edit::SyntaxKind::RIGHT_BRACE,
        ) => previous_significant,
        _ => None,
    };
    (
        previous_significant,
        previous_significant == Some(yaml_edit::SyntaxKind::STRING) && previous_string_is_quoted,
    )
}

fn yaml_lex_contains_structural_anchor_or_alias(source: &str) -> bool {
    let mut previous_significant = None;
    let mut at_line_start = true;
    let mut line_indent = 0;
    let mut flow_depth = 0;
    let mut block_scalar_base_indent: Option<usize> = None;
    let mut block_scalar_content_indent: Option<usize> = None;
    let mut block_scalar_explicit_indent: Option<usize> = None;
    let mut previous_string_is_quoted = false;
    let mut previous_scalar_line_indent = None;
    let mut plain_scalar_continuation = false;
    let mut plain_scalar_continuation_marker_seen = false;
    let mut line_has_sequence_marker = false;
    let mut line_has_sequence_mapping = false;
    let mut line_column: usize = 0;
    let mut compact_sequence_key_pending = false;
    let mut compact_sequence_key_column = None;

    for (kind, text) in yaml_edit::lex(source) {
        let token_column = line_column;
        if at_line_start {
            match kind {
                yaml_edit::SyntaxKind::NEWLINE => {
                    line_indent = 0;
                    line_has_sequence_marker = false;
                    line_has_sequence_mapping = false;
                    line_column = 0;
                    compact_sequence_key_pending = false;
                    compact_sequence_key_column = None;
                    continue;
                }
                yaml_edit::SyntaxKind::INDENT => {
                    line_indent = text.chars().count();
                    line_column = line_column.saturating_add(text.chars().count());
                    continue;
                }
                _ => {
                    if let Some(base_indent) = block_scalar_base_indent {
                        if let Some(content_indent) = block_scalar_content_indent {
                            if line_indent < content_indent {
                                block_scalar_base_indent = None;
                                block_scalar_content_indent = None;
                                block_scalar_explicit_indent = None;
                            }
                        } else if let Some(explicit_indent) = block_scalar_explicit_indent {
                            let required_indent = base_indent.saturating_add(explicit_indent);
                            if line_indent < required_indent {
                                block_scalar_base_indent = None;
                                block_scalar_explicit_indent = None;
                            } else {
                                block_scalar_content_indent = Some(required_indent);
                                block_scalar_explicit_indent = None;
                            }
                        } else if line_indent <= base_indent {
                            block_scalar_base_indent = None;
                        } else {
                            block_scalar_content_indent = Some(line_indent);
                        }
                    }
                    plain_scalar_continuation = block_scalar_base_indent.is_none()
                        && flow_depth == 0
                        && previous_significant.is_some_and(|kind| {
                            matches!(
                                kind,
                                yaml_edit::SyntaxKind::STRING
                                    | yaml_edit::SyntaxKind::INT
                                    | yaml_edit::SyntaxKind::FLOAT
                                    | yaml_edit::SyntaxKind::BOOL
                                    | yaml_edit::SyntaxKind::NULL
                            )
                        })
                        && !previous_string_is_quoted
                        && previous_scalar_line_indent.is_some_and(|indent| line_indent > indent);
                    plain_scalar_continuation_marker_seen = false;
                    if !plain_scalar_continuation
                        && previous_significant.is_some_and(|kind| {
                            matches!(
                                kind,
                                yaml_edit::SyntaxKind::STRING
                                    | yaml_edit::SyntaxKind::INT
                                    | yaml_edit::SyntaxKind::FLOAT
                                    | yaml_edit::SyntaxKind::BOOL
                                    | yaml_edit::SyntaxKind::NULL
                            )
                        })
                        && !previous_string_is_quoted
                        && previous_scalar_line_indent.is_some_and(|indent| line_indent <= indent)
                    {
                        previous_significant = None;
                        previous_string_is_quoted = false;
                        previous_scalar_line_indent = None;
                    }
                    at_line_start = false;
                }
            }
        }

        if kind == yaml_edit::SyntaxKind::NEWLINE {
            at_line_start = true;
            line_indent = 0;
            line_has_sequence_marker = false;
            line_has_sequence_mapping = false;
            line_column = 0;
            compact_sequence_key_pending = false;
            compact_sequence_key_column = None;
            (previous_significant, previous_string_is_quoted) =
                yaml_previous_significant_after_newline(
                    previous_significant,
                    previous_string_is_quoted,
                );
            if !previous_significant.is_some_and(|kind| {
                matches!(
                    kind,
                    yaml_edit::SyntaxKind::STRING
                        | yaml_edit::SyntaxKind::INT
                        | yaml_edit::SyntaxKind::FLOAT
                        | yaml_edit::SyntaxKind::BOOL
                        | yaml_edit::SyntaxKind::NULL
                )
            }) {
                previous_scalar_line_indent = None;
            }
            plain_scalar_continuation = false;
            plain_scalar_continuation_marker_seen = false;
            continue;
        }

        if block_scalar_base_indent.is_some() {
            if block_scalar_content_indent.is_none()
                && block_scalar_explicit_indent.is_none()
                && let Some(indent) = yaml_explicit_block_scalar_indent(text)
            {
                block_scalar_explicit_indent = Some(indent);
            }
            line_column = line_column.saturating_add(text.chars().count());
            continue;
        }

        if kind == yaml_edit::SyntaxKind::COMMENT {
            previous_significant = None;
            previous_string_is_quoted = false;
            previous_scalar_line_indent = None;
            plain_scalar_continuation = false;
            plain_scalar_continuation_marker_seen = false;
            line_column = line_column.saturating_add(text.chars().count());
            continue;
        }

        if kind == yaml_edit::SyntaxKind::STRING && text == "-" && line_has_sequence_marker {
            compact_sequence_key_column = Some(token_column);
            compact_sequence_key_pending = true;
        } else if compact_sequence_key_pending
            && matches!(
                kind,
                yaml_edit::SyntaxKind::STRING
                    | yaml_edit::SyntaxKind::INT
                    | yaml_edit::SyntaxKind::FLOAT
                    | yaml_edit::SyntaxKind::BOOL
                    | yaml_edit::SyntaxKind::NULL
            )
        {
            compact_sequence_key_column = Some(token_column);
            compact_sequence_key_pending = false;
        }

        if plain_scalar_continuation {
            match kind {
                yaml_edit::SyntaxKind::TAG
                | yaml_edit::SyntaxKind::ANCHOR
                | yaml_edit::SyntaxKind::REFERENCE => {
                    plain_scalar_continuation_marker_seen = true;
                    line_column = line_column.saturating_add(text.chars().count());
                    continue;
                }
                yaml_edit::SyntaxKind::COLON => {
                    if plain_scalar_continuation_marker_seen {
                        return true;
                    }
                    plain_scalar_continuation = false;
                }
                _ => {
                    line_column = line_column.saturating_add(text.chars().count());
                    continue;
                }
            }
        }

        let structural_property = yaml_property_token_is_structural(
            kind,
            previous_significant,
            previous_string_is_quoted,
        );
        let structural_flow = yaml_flow_token_is_structural(
            kind,
            flow_depth,
            previous_significant,
            previous_string_is_quoted,
        );
        let structural_block_scalar = yaml_block_scalar_indicator_is_structural(
            kind,
            previous_significant,
            previous_string_is_quoted,
        );
        if matches!(
            kind,
            yaml_edit::SyntaxKind::ANCHOR | yaml_edit::SyntaxKind::REFERENCE
        ) && structural_property
        {
            return true;
        }

        if structural_block_scalar {
            let base_indent = if line_has_sequence_mapping {
                compact_sequence_key_column.unwrap_or_else(|| line_indent.saturating_add(2))
            } else {
                compact_sequence_key_column.unwrap_or(line_indent)
            };
            block_scalar_base_indent = Some(base_indent);
            block_scalar_content_indent = None;
            block_scalar_explicit_indent = None;
        }

        match kind {
            yaml_edit::SyntaxKind::DASH => {
                line_has_sequence_marker = true;
                compact_sequence_key_pending = true;
                compact_sequence_key_column = None;
            }
            yaml_edit::SyntaxKind::STRING if text == "-" && line_has_sequence_marker => {
                line_has_sequence_marker = true;
            }
            yaml_edit::SyntaxKind::COLON if line_has_sequence_marker => {
                line_has_sequence_mapping = true;
            }
            yaml_edit::SyntaxKind::LEFT_BRACKET | yaml_edit::SyntaxKind::LEFT_BRACE
                if structural_flow =>
            {
                flow_depth = flow_depth.saturating_add(1);
            }
            yaml_edit::SyntaxKind::RIGHT_BRACKET | yaml_edit::SyntaxKind::RIGHT_BRACE
                if structural_flow =>
            {
                flow_depth = flow_depth.saturating_sub(1);
            }
            _ => {}
        }

        if (!matches!(
            kind,
            yaml_edit::SyntaxKind::TAG
                | yaml_edit::SyntaxKind::ANCHOR
                | yaml_edit::SyntaxKind::REFERENCE
        ) || structural_property)
            && (!matches!(
                kind,
                yaml_edit::SyntaxKind::LEFT_BRACKET
                    | yaml_edit::SyntaxKind::LEFT_BRACE
                    | yaml_edit::SyntaxKind::RIGHT_BRACKET
                    | yaml_edit::SyntaxKind::RIGHT_BRACE
            ) || structural_flow)
            && !matches!(
                kind,
                yaml_edit::SyntaxKind::WHITESPACE
                    | yaml_edit::SyntaxKind::INDENT
                    | yaml_edit::SyntaxKind::COMMENT
                    | yaml_edit::SyntaxKind::BOM
            )
        {
            if matches!(
                kind,
                yaml_edit::SyntaxKind::STRING
                    | yaml_edit::SyntaxKind::INT
                    | yaml_edit::SyntaxKind::FLOAT
                    | yaml_edit::SyntaxKind::BOOL
                    | yaml_edit::SyntaxKind::NULL
            ) {
                previous_string_is_quoted = kind == yaml_edit::SyntaxKind::STRING
                    && (text.starts_with('\'') || text.starts_with('"'));
                previous_scalar_line_indent = Some(line_indent);
            } else {
                previous_string_is_quoted = false;
                previous_scalar_line_indent = None;
            }
            previous_significant = Some(
                if kind == yaml_edit::SyntaxKind::STRING && text == "-" && line_has_sequence_marker
                {
                    yaml_edit::SyntaxKind::DASH
                } else {
                    kind
                },
            );
        }
        line_column = line_column.saturating_add(text.chars().count());
    }

    false
}

fn yaml_lex_exceeds_nesting(source: &str) -> bool {
    if yaml_has_excessive_blank_line_run(source) {
        return true;
    }

    let mut at_line_start = true;
    let mut line_indent: usize = 0;
    let mut line_dash_count: usize = 0;
    let mut flow_depth: usize = 0;
    let mut flow_line_breaks: Vec<usize> = Vec::new();
    let mut block_scalar_base_indent: Option<usize> = None;
    let mut block_scalar_content_indent: Option<usize> = None;
    let mut block_scalar_explicit_indent: Option<usize> = None;
    let mut block_indents = Vec::new();
    let mut sequence_prefixes: Vec<(usize, usize)> = Vec::new();
    let mut tag_chain_depth: usize = 0;
    let mut node_property_count: usize = 0;
    let mut previous_significant = None;
    let mut previous_string_is_quoted = false;
    let mut previous_scalar_line_indent = None;
    let mut plain_scalar_continuation = false;
    let mut plain_scalar_continuation_marker_seen = false;
    let mut line_has_sequence_marker = false;
    let mut line_has_sequence_mapping = false;
    let mut line_column: usize = 0;
    let mut compact_sequence_key_pending = false;
    let mut compact_sequence_key_column = None;

    for (kind, text) in yaml_edit::lex(source) {
        let token_column = line_column;
        if kind == yaml_edit::SyntaxKind::NEWLINE
            && flow_depth > 0
            && block_scalar_base_indent.is_none()
        {
            for line_breaks in &mut flow_line_breaks {
                *line_breaks = line_breaks.saturating_add(1);
                if *line_breaks > MAX_YAML_NESTING_DEPTH {
                    return true;
                }
            }
        }

        if at_line_start {
            match kind {
                yaml_edit::SyntaxKind::NEWLINE => {
                    line_indent = 0;
                    line_has_sequence_marker = false;
                    line_has_sequence_mapping = false;
                    line_column = 0;
                    compact_sequence_key_pending = false;
                    compact_sequence_key_column = None;
                    continue;
                }
                yaml_edit::SyntaxKind::INDENT => {
                    line_indent = text.chars().count();
                    line_column = line_column.saturating_add(text.chars().count());
                    continue;
                }
                _ => {
                    if let Some(base_indent) = block_scalar_base_indent {
                        if let Some(content_indent) = block_scalar_content_indent {
                            if line_indent < content_indent {
                                block_scalar_base_indent = None;
                                block_scalar_content_indent = None;
                                block_scalar_explicit_indent = None;
                            }
                        } else if let Some(explicit_indent) = block_scalar_explicit_indent {
                            let required_indent = base_indent.saturating_add(explicit_indent);
                            if line_indent < required_indent {
                                block_scalar_base_indent = None;
                                block_scalar_explicit_indent = None;
                            } else {
                                block_scalar_content_indent = Some(required_indent);
                                block_scalar_explicit_indent = None;
                            }
                        } else if line_indent <= base_indent {
                            block_scalar_base_indent = None;
                        } else {
                            block_scalar_content_indent = Some(line_indent);
                        }
                    }
                    plain_scalar_continuation = block_scalar_base_indent.is_none()
                        && flow_depth == 0
                        && previous_significant.is_some_and(|kind| {
                            matches!(
                                kind,
                                yaml_edit::SyntaxKind::STRING
                                    | yaml_edit::SyntaxKind::INT
                                    | yaml_edit::SyntaxKind::FLOAT
                                    | yaml_edit::SyntaxKind::BOOL
                                    | yaml_edit::SyntaxKind::NULL
                            )
                        })
                        && !previous_string_is_quoted
                        && previous_scalar_line_indent.is_some_and(|indent| line_indent > indent);
                    plain_scalar_continuation_marker_seen = false;
                    if !plain_scalar_continuation
                        && previous_significant.is_some_and(|kind| {
                            matches!(
                                kind,
                                yaml_edit::SyntaxKind::STRING
                                    | yaml_edit::SyntaxKind::INT
                                    | yaml_edit::SyntaxKind::FLOAT
                                    | yaml_edit::SyntaxKind::BOOL
                                    | yaml_edit::SyntaxKind::NULL
                            )
                        })
                        && !previous_string_is_quoted
                        && previous_scalar_line_indent.is_some_and(|indent| line_indent <= indent)
                    {
                        previous_significant = None;
                        previous_string_is_quoted = false;
                        previous_scalar_line_indent = None;
                    }
                    at_line_start = false;
                    line_dash_count = 0;
                    while sequence_prefixes
                        .last()
                        .is_some_and(|(indent, _)| *indent > line_indent)
                    {
                        sequence_prefixes.pop();
                    }
                }
            }
        }

        if kind == yaml_edit::SyntaxKind::NEWLINE {
            at_line_start = true;
            line_indent = 0;
            line_dash_count = 0;
            line_has_sequence_marker = false;
            line_has_sequence_mapping = false;
            line_column = 0;
            compact_sequence_key_pending = false;
            compact_sequence_key_column = None;
            (previous_significant, previous_string_is_quoted) =
                yaml_previous_significant_after_newline(
                    previous_significant,
                    previous_string_is_quoted,
                );
            if !previous_significant.is_some_and(|kind| {
                matches!(
                    kind,
                    yaml_edit::SyntaxKind::STRING
                        | yaml_edit::SyntaxKind::INT
                        | yaml_edit::SyntaxKind::FLOAT
                        | yaml_edit::SyntaxKind::BOOL
                        | yaml_edit::SyntaxKind::NULL
                )
            }) {
                previous_scalar_line_indent = None;
            }
            plain_scalar_continuation = false;
            plain_scalar_continuation_marker_seen = false;
            continue;
        }

        if block_scalar_base_indent.is_some() {
            if block_scalar_content_indent.is_none()
                && block_scalar_explicit_indent.is_none()
                && let Some(indent) = yaml_explicit_block_scalar_indent(text)
            {
                block_scalar_explicit_indent = Some(indent);
            }
            line_column = line_column.saturating_add(text.chars().count());
            continue;
        }

        if kind == yaml_edit::SyntaxKind::COMMENT {
            previous_significant = None;
            previous_string_is_quoted = false;
            previous_scalar_line_indent = None;
            plain_scalar_continuation = false;
            plain_scalar_continuation_marker_seen = false;
            line_column = line_column.saturating_add(text.chars().count());
            continue;
        }

        if kind == yaml_edit::SyntaxKind::STRING && text == "-" && line_has_sequence_marker {
            compact_sequence_key_column = Some(token_column);
            compact_sequence_key_pending = true;
        } else if compact_sequence_key_pending
            && matches!(
                kind,
                yaml_edit::SyntaxKind::STRING
                    | yaml_edit::SyntaxKind::INT
                    | yaml_edit::SyntaxKind::FLOAT
                    | yaml_edit::SyntaxKind::BOOL
                    | yaml_edit::SyntaxKind::NULL
            )
        {
            compact_sequence_key_column = Some(token_column);
            compact_sequence_key_pending = false;
        }

        if plain_scalar_continuation {
            match kind {
                yaml_edit::SyntaxKind::TAG
                | yaml_edit::SyntaxKind::ANCHOR
                | yaml_edit::SyntaxKind::REFERENCE => {
                    plain_scalar_continuation_marker_seen = true;
                    line_column = line_column.saturating_add(text.chars().count());
                    continue;
                }
                yaml_edit::SyntaxKind::COLON => {
                    if plain_scalar_continuation_marker_seen {
                        return true;
                    }
                    plain_scalar_continuation = false;
                }
                _ => {
                    line_column = line_column.saturating_add(text.chars().count());
                    continue;
                }
            }
        }

        let structural_property = yaml_property_token_is_structural(
            kind,
            previous_significant,
            previous_string_is_quoted,
        );
        let structural_flow = yaml_flow_token_is_structural(
            kind,
            flow_depth,
            previous_significant,
            previous_string_is_quoted,
        );
        let structural_block_scalar = yaml_block_scalar_indicator_is_structural(
            kind,
            previous_significant,
            previous_string_is_quoted,
        );
        if matches!(
            kind,
            yaml_edit::SyntaxKind::TAG
                | yaml_edit::SyntaxKind::ANCHOR
                | yaml_edit::SyntaxKind::REFERENCE
        ) && structural_property
        {
            node_property_count = node_property_count.saturating_add(1);
            if node_property_count > MAX_YAML_NESTING_DEPTH {
                return true;
            }
        }

        while block_indents
            .last()
            .is_some_and(|indent| *indent > line_indent)
        {
            block_indents.pop();
        }
        if line_indent > block_indents.last().copied().unwrap_or_default() {
            block_indents.push(line_indent);
        }

        match kind {
            yaml_edit::SyntaxKind::DASH => {
                line_has_sequence_marker = true;
                compact_sequence_key_pending = true;
                compact_sequence_key_column = None;
                line_dash_count = line_dash_count.saturating_add(1);
                if let Some((_, count)) = sequence_prefixes
                    .iter_mut()
                    .find(|(indent, _)| *indent == line_indent)
                {
                    *count = line_dash_count;
                } else {
                    sequence_prefixes.push((line_indent, line_dash_count));
                }
            }
            yaml_edit::SyntaxKind::STRING if text == "-" && line_has_sequence_marker => {
                line_dash_count = line_dash_count.saturating_add(1);
                if let Some((_, count)) = sequence_prefixes
                    .iter_mut()
                    .find(|(indent, _)| *indent == line_indent)
                {
                    *count = line_dash_count;
                } else {
                    sequence_prefixes.push((line_indent, line_dash_count));
                }
            }
            yaml_edit::SyntaxKind::COLON if line_has_sequence_marker => {
                line_has_sequence_mapping = true;
            }
            yaml_edit::SyntaxKind::LEFT_BRACKET | yaml_edit::SyntaxKind::LEFT_BRACE
                if structural_flow =>
            {
                flow_depth = flow_depth.saturating_add(1);
                flow_line_breaks.push(0);
            }
            yaml_edit::SyntaxKind::RIGHT_BRACKET | yaml_edit::SyntaxKind::RIGHT_BRACE
                if structural_flow =>
            {
                flow_depth = flow_depth.saturating_sub(1);
                flow_line_breaks.pop();
            }
            yaml_edit::SyntaxKind::TAG if structural_property => {
                tag_chain_depth = tag_chain_depth.saturating_add(1);
            }
            yaml_edit::SyntaxKind::PIPE | yaml_edit::SyntaxKind::GREATER
                if structural_block_scalar =>
            {
                let base_indent = if line_has_sequence_mapping {
                    compact_sequence_key_column.unwrap_or_else(|| line_indent.saturating_add(2))
                } else {
                    compact_sequence_key_column.unwrap_or(line_indent)
                };
                block_scalar_base_indent = Some(base_indent);
                block_scalar_content_indent = None;
                block_scalar_explicit_indent = None;
            }
            yaml_edit::SyntaxKind::WHITESPACE
            | yaml_edit::SyntaxKind::INDENT
            | yaml_edit::SyntaxKind::COMMENT
            | yaml_edit::SyntaxKind::BOM => {}
            _ => {}
        }

        if (!matches!(
            kind,
            yaml_edit::SyntaxKind::TAG
                | yaml_edit::SyntaxKind::ANCHOR
                | yaml_edit::SyntaxKind::REFERENCE
        ) || structural_property)
            && (!matches!(
                kind,
                yaml_edit::SyntaxKind::LEFT_BRACKET
                    | yaml_edit::SyntaxKind::LEFT_BRACE
                    | yaml_edit::SyntaxKind::RIGHT_BRACKET
                    | yaml_edit::SyntaxKind::RIGHT_BRACE
            ) || structural_flow)
            && !matches!(
                kind,
                yaml_edit::SyntaxKind::WHITESPACE
                    | yaml_edit::SyntaxKind::INDENT
                    | yaml_edit::SyntaxKind::COMMENT
                    | yaml_edit::SyntaxKind::BOM
            )
        {
            if matches!(
                kind,
                yaml_edit::SyntaxKind::STRING
                    | yaml_edit::SyntaxKind::INT
                    | yaml_edit::SyntaxKind::FLOAT
                    | yaml_edit::SyntaxKind::BOOL
                    | yaml_edit::SyntaxKind::NULL
            ) {
                previous_string_is_quoted = kind == yaml_edit::SyntaxKind::STRING
                    && (text.starts_with('\'') || text.starts_with('"'));
                previous_scalar_line_indent = Some(line_indent);
            } else {
                previous_string_is_quoted = false;
                previous_scalar_line_indent = None;
            }
            previous_significant = Some(
                if kind == yaml_edit::SyntaxKind::STRING && text == "-" && line_has_sequence_marker
                {
                    yaml_edit::SyntaxKind::DASH
                } else {
                    kind
                },
            );
        }

        let sequence_depth = sequence_prefixes
            .iter()
            .map(|(_, depth)| *depth)
            .sum::<usize>();
        if flow_depth
            .saturating_add(block_indents.len())
            .saturating_add(sequence_depth)
            .saturating_add(tag_chain_depth)
            > MAX_YAML_NESTING_DEPTH
        {
            return true;
        }
        line_column = line_column.saturating_add(text.chars().count());
    }

    false
}

fn yaml_has_excessive_blank_line_run(source: &str) -> bool {
    if source.is_empty() {
        return false;
    }

    let bytes = source.as_bytes();
    let mut blank_line_run = 0;
    let mut line_start = 0;
    let mut index = 0;
    loop {
        while index < bytes.len() && !matches!(bytes[index], b'\n' | b'\r') {
            index += 1;
        }
        let line = &source[line_start..index];
        if line.trim().is_empty() {
            blank_line_run += 1;
            if blank_line_run > MAX_YAML_NESTING_DEPTH {
                return true;
            }
        } else {
            blank_line_run = 0;
        }

        if index == bytes.len() {
            break;
        }
        if bytes[index] == b'\r' {
            index += 1;
            if index < bytes.len() && bytes[index] == b'\n' {
                index += 1;
            }
        } else {
            index += 1;
        }
        line_start = index;
        if line_start == bytes.len() {
            break;
        }
    }
    false
}

fn source_is_yaml_candidate(source: &str) -> bool {
    let lines = meaningful_lines(source);
    if lines.is_empty() || lines.iter().all(|line| is_uri_line(line)) {
        return false;
    }
    let trimmed = source.trim_start_matches('\u{feff}').trim_start();
    trimmed.starts_with("---")
        || trimmed.starts_with('{')
        || trimmed.starts_with('[')
        || trimmed.starts_with('&')
        || trimmed.starts_with('*')
        || lines.iter().any(|line| {
            line.starts_with("---")
                || line.starts_with('{')
                || line.starts_with('[')
                || line.starts_with('&')
                || line.starts_with('*')
                || line.contains(':')
                || line
                    .chars()
                    .nth(1)
                    .is_some_and(|value| value.is_ascii_whitespace())
        })
}

fn is_uri_line(line: &str) -> bool {
    let Some((scheme, _)) = line.split_once("://") else {
        return false;
    };
    !scheme.is_empty()
        && scheme
            .chars()
            .all(|value| value.is_ascii_alphanumeric() || matches!(value, '+' | '-' | '.'))
        && !line.chars().any(|value| value.is_ascii_whitespace())
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
    reject_yaml_aliases(source)?;
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

    let root_warning = map
        .keys()
        .any(|key| !matches!(key.as_str(), Some("proxies")))
        .then(|| "ignored_root_fields".to_owned());

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
        Some(Value::String(name)) if name.trim().is_empty() => None,
        Some(Value::String(name)) if uri::is_sensitive_name(name) => None,
        Some(Value::String(name)) => match uri::safe_name(name) {
            Some(name) => Some(name),
            None => {
                return Candidate {
                    scheme: yaml_scheme(map),
                    name: None,
                    proxy: None,
                    warnings: Vec::new(),
                    failure: Some("proxy name is invalid".to_owned()),
                };
            }
        },
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
            scheme: yaml_scheme(map),
            name,
            proxy: None,
            warnings: Vec::new(),
            failure: Some(reason),
        };
    }

    Candidate {
        scheme: yaml_scheme(map),
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
        .map(|value| uri::safe_label(&value).unwrap_or_else(|| "unknown".to_owned()))
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
        "vless" | "vmess" => &["uuid"],
        "ss" => &["cipher", "password"],
        "trojan" | "hysteria2" | "hy2" => &["password"],
        _ => &[],
    };
    for field in required {
        if !yaml_nonempty_string(map, field) {
            return Some(format!("{scheme} {field} is required"));
        }
    }
    if scheme == "tuic"
        && !yaml_nonempty_string(map, "token")
        && !(yaml_nonempty_string(map, "uuid") && yaml_nonempty_string(map, "password"))
    {
        return Some("tuic token or uuid/password is required".to_owned());
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
    reject_yaml_aliases(input)?;
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

fn ensure_appendable_extra_proxies_yaml(
    existing: &str,
    added_count: usize,
    added_yaml: &str,
) -> Result<(), ImportError> {
    if added_count == 0 || existing.trim().is_empty() {
        return Ok(());
    }
    let separator = if !existing.ends_with('\n') { "\n" } else { "" };
    let merged = format!("{existing}{separator}{added_yaml}");
    let value = serde_yaml::from_str::<Value>(&merged).map_err(|_| {
        ImportError::invalid(
            "existing_extra_proxies_yaml must be a block sequence that accepts appended nodes",
        )
    })?;
    if !matches!(value, Value::Sequence(_)) {
        return Err(ImportError::invalid(
            "existing_extra_proxies_yaml must be a block sequence that accepts appended nodes",
        ));
    }
    Ok(())
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

    let mut names = import_names::ImportNameState::new(reserved_names, &existing);
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

        let source_name = import_names::source_name(&proxy).or_else(|| candidate.name.clone());
        let pending_dialer_proxy = names.remap_known_dialer_proxy(&mut proxy);
        let fingerprint = canonical_proxy(&proxy);
        if !pending_dialer_proxy && names.is_duplicate(&fingerprint, source_name.as_deref()) {
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
            .clone()
            .unwrap_or_else(|| format!("imported-{source_index:03}"));
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
        let final_name = names.allocate(&base_name);
        if final_name != base_name {
            warnings.push("name_collision_suffix_allocated".to_owned());
            counts.warnings += 1;
        }
        proxy = assign_name(proxy, &final_name);
        names.defer_dialer_proxy(pending_dialer_proxy, &final_name);
        names.accept(fingerprint, source_name.as_deref(), final_name.clone());
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

    names.remap_pending_dialer_proxies(&mut accepted, &mut items, &mut counts);

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
        Value::Null => serde_json::json!({"kind": "null"}),
        Value::Bool(value) => serde_json::json!({"kind": "bool", "value": value}),
        Value::Number(value) => {
            serde_json::json!({"kind": "number", "value": value.to_string()})
        }
        Value::String(value) => serde_json::json!({"kind": "string", "value": value}),
        Value::Sequence(values) => serde_json::json!({
            "kind": "sequence",
            "value": values
                .iter()
                .map(|value| canonical_value(value, false))
                .collect::<Vec<_>>(),
        }),
        Value::Mapping(map) => {
            let mut entries = Vec::new();
            for (key, value) in map {
                if ignore_name && matches!(key, Value::String(key) if key == "name") {
                    continue;
                }
                let canonical_key = canonical_value(key, false);
                let canonical_entry = serde_json::json!({
                    "key": canonical_key.clone(),
                    "value": canonical_value(value, false),
                });
                let sort_key = serde_json::to_string(&canonical_key).unwrap_or_default();
                entries.push((sort_key, canonical_entry));
            }
            entries.sort_by(|left, right| left.0.cmp(&right.0));
            serde_json::json!({
                "kind": "mapping",
                "value": entries
                    .into_iter()
                    .map(|(_, entry)| entry)
                    .collect::<Vec<_>>(),
            })
        }
        Value::Tagged(tagged) => serde_json::json!({
            "kind": "tagged",
            "tag": tagged.tag.to_string(),
            "value": canonical_value(&tagged.value, ignore_name),
        }),
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
    fn normalizes_bom_and_carriage_return_uri_lines() {
        let source = concat!(
            "\u{feff}vless://00000000-0000-0000-0000-000000000002@vless.example:443\r",
            "ss://YWVzLTI1Ni1nY206cGFzc3dvcmQ@example.com:443\r",
        );
        let preview = preview_text(source, "", &empty_reserved()).expect("preview");
        assert_eq!(preview.counts.added, 2);
        assert_eq!(preview.counts.failed, 0);
    }

    #[test]
    fn rejects_excessive_yaml_nesting_before_deserialization() {
        let depth = super::MAX_YAML_NESTING_DEPTH + 1;
        let source = format!("proxies: {}{}\n", "[".repeat(depth), "]".repeat(depth));
        let error = preview_text(&source, "", &empty_reserved()).expect_err("depth bound");
        assert!(matches!(error, super::ImportError::Invalid(_)));
    }

    #[test]
    fn rejects_excessive_yaml_tag_nesting_before_deserialization() {
        let depth = super::MAX_YAML_NESTING_DEPTH + 1;
        let source = format!("proxies: {}vendor-private\n", "!tag ".repeat(depth));
        let error = preview_text(&source, "", &empty_reserved()).expect_err("tag depth bound");
        assert!(matches!(error, super::ImportError::Invalid(_)));
    }

    #[test]
    fn rejects_excessive_multiline_yaml_tag_nesting_before_deserialization() {
        let depth = super::MAX_YAML_NESTING_DEPTH + 1;
        let source = format!(
            "proxies: !tag\n{}vendor-private\n",
            "  !tag\n".repeat(depth)
        );
        let error = preview_text(&source, "", &empty_reserved()).expect_err("multiline tag bound");
        assert!(matches!(error, super::ImportError::Invalid(_)));
    }

    #[test]
    fn rejects_excessive_multiline_compact_sequence_nesting() {
        let mut source = String::from("proxies:\n");
        for level in 0..=super::MAX_YAML_NESTING_DEPTH {
            source.push_str(&"  ".repeat(level));
            source.push_str("- -\n");
        }
        let error = preview_text(&source, "", &empty_reserved()).expect_err("sequence depth bound");
        assert!(matches!(error, super::ImportError::Invalid(_)));
    }

    #[test]
    fn accepts_plain_flow_scalars_with_embedded_quote_and_ampersand() {
        let preview = preview_text(
            r#"proxies: [{type: vendor-private, password: a"b&c}]"#,
            "",
            &empty_reserved(),
        )
        .expect("plain flow scalar");
        assert_eq!(preview.counts.added, 1);
    }

    #[test]
    fn accepts_multiline_plain_scalars_with_embedded_marker_text() {
        let preview = preview_text(
            "proxies:\n  - type: vendor-private\n    password: hello\n      &literal\n",
            "",
            &empty_reserved(),
        )
        .expect("multiline plain scalar");
        assert_eq!(preview.counts.added, 1);
    }

    #[test]
    fn accepts_many_marker_like_multiline_plain_scalar_lines() {
        let mut source = String::from("proxies:\n  - type: vendor-private\n    password: hello\n");
        for _ in 0..=super::MAX_YAML_NESTING_DEPTH {
            source.push_str("      &literal text\n");
        }
        let preview =
            preview_text(&source, "", &empty_reserved()).expect("plain scalar marker text");
        assert_eq!(preview.counts.added, 1);
    }

    #[test]
    fn accepts_many_flow_marker_like_multiline_plain_scalar_lines() {
        let mut source = String::from("proxies:\n  - type: vendor-private\n    password: hello\n");
        for _ in 0..=super::MAX_YAML_NESTING_DEPTH {
            source.push_str("      [literal\n");
        }
        let preview = preview_text(&source, "", &empty_reserved()).expect("plain scalar flow text");
        assert_eq!(preview.counts.added, 1);
    }

    #[test]
    fn rejects_excessive_mixed_yaml_property_chain_before_deserialization() {
        let depth = super::MAX_YAML_NESTING_DEPTH + 1;
        let mut source = String::from("proxies: ");
        for index in 0..depth {
            if index % 2 == 0 {
                source.push_str("&anchor ");
            } else {
                source.push_str("!tag ");
            }
        }
        source.push_str("vendor-private\n");
        let error = preview_text(&source, "", &empty_reserved()).expect_err("property chain bound");
        assert!(matches!(error, super::ImportError::Invalid(_)));
    }

    #[test]
    fn rejects_excessive_explicit_key_property_chain_before_deserialization() {
        let depth = super::MAX_YAML_NESTING_DEPTH + 1;
        let mut source = String::from("proxies: ? ");
        for index in 0..depth {
            if index % 2 == 0 {
                source.push_str("&anchor ");
            } else {
                source.push_str("!tag ");
            }
        }
        source.push_str("vendor-private\n");
        let error = preview_text(&source, "", &empty_reserved()).expect_err("explicit key bound");
        assert!(matches!(error, super::ImportError::Invalid(_)));
    }

    #[test]
    fn rejects_excessive_reference_followed_property_chain_before_deserialization() {
        let depth = super::MAX_YAML_NESTING_DEPTH + 1;
        let source = format!(
            "proxies: [*reference {}vendor-private]\n",
            "!tag ".repeat(depth)
        );
        let error =
            preview_text(&source, "", &empty_reserved()).expect_err("reference chain bound");
        assert!(matches!(error, super::ImportError::Invalid(_)));
    }

    #[test]
    fn rejects_excessive_property_chain_after_a_closed_flow_value_before_deserialization() {
        let depth = super::MAX_YAML_NESTING_DEPTH + 1;
        let source = format!(
            "proxies: [[vendor-private] {}vendor-private]\n",
            "!tag ".repeat(depth)
        );
        let error = preview_text(&source, "", &empty_reserved()).expect_err("closed flow bound");
        assert!(matches!(error, super::ImportError::Invalid(_)));
    }

    #[test]
    fn rejects_excessive_property_chain_after_a_quoted_flow_value_before_deserialization() {
        let depth = super::MAX_YAML_NESTING_DEPTH + 1;
        let source = format!(
            "proxies: [\"vendor-private\" {}vendor-private]\n",
            "!tag ".repeat(depth)
        );
        let error = preview_text(&source, "", &empty_reserved()).expect_err("quoted flow bound");
        assert!(matches!(error, super::ImportError::Invalid(_)));
    }

    #[test]
    fn rejects_excessive_property_chain_after_a_commented_flow_value_before_deserialization() {
        let depth = super::MAX_YAML_NESTING_DEPTH + 1;
        let source = format!(
            "proxies: [vendor-private # end the scalar\n{}vendor-private]\n",
            "!tag ".repeat(depth)
        );
        let error = preview_text(&source, "", &empty_reserved()).expect_err("commented flow bound");
        assert!(matches!(error, super::ImportError::Invalid(_)));
    }

    #[test]
    fn rejects_excessive_property_chain_after_blank_lines_before_deserialization() {
        let depth = super::MAX_YAML_NESTING_DEPTH + 1;
        let mut source = String::from("proxies: [vendor-private\n");
        for _ in 0..depth {
            source.push('\n');
            source.push_str("!tag\n");
        }
        source.push_str("vendor-private]\n");
        let error = preview_text(&source, "", &empty_reserved()).expect_err("blank line bound");
        assert!(matches!(error, super::ImportError::Invalid(_)));
    }

    #[test]
    fn rejects_excessive_flow_line_breaks_before_deserialization() {
        let groups = super::MAX_YAML_NESTING_DEPTH / 2 + 1;
        let mut source = String::from("proxies: [vendor-private\n");
        for _ in 0..groups {
            source.push('\n');
            source.push_str("!tag\n");
        }
        source.push_str("vendor-private]\n");
        let error = preview_text(&source, "", &empty_reserved()).expect_err("flow line bound");
        assert!(matches!(error, super::ImportError::Invalid(_)));
    }

    #[test]
    fn rejects_nested_flow_line_breaks_against_the_outer_budget() {
        let groups = super::MAX_YAML_NESTING_DEPTH / 2 + 1;
        let mut source = String::from("proxies: [\n");
        for _ in 0..groups {
            source.push_str("  [\n    vendor-private\n  ],\n");
        }
        source.push_str("]\n");
        assert!(super::yaml_lex_exceeds_nesting(&source));
    }

    #[test]
    fn accepts_marker_like_block_scalar_text_after_a_blank_line() {
        let source = concat!(
            "proxies:\n",
            "  - type: vendor-private\n",
            "    password: hello\n",
            "\n",
            "      &literal text\n",
        );
        let preview = preview_text(source, "", &empty_reserved()).expect("plain scalar text");
        assert_eq!(preview.counts.added, 1);
    }

    #[test]
    fn accepts_marker_like_text_in_explicitly_indented_block_scalars() {
        for indicator in ["|2", "|2-", "|-2", ">2-", ">-2"] {
            let source = format!(
                "proxies:\n  - description: {indicator}\n        &literal first\n      *literal second\n    type: vendor-private\n"
            );
            let preview =
                preview_text(&source, "", &empty_reserved()).expect("explicit indentation");
            assert_eq!(preview.counts.added, 1, "indicator {indicator}");
        }

        for indicator in ["|2", "|2-", "|-2", ">2-", ">-2"] {
            let source = format!(
                concat!(
                    "ignored:\n",
                    "  - {}\n",
                    "    &literal first\n",
                    "    *literal second\n",
                    "proxies:\n",
                    "  - type: vendor-private\n",
                    "    server: example.com\n",
                    "    port: 443\n",
                ),
                indicator
            );
            preview_text(&source, "", &empty_reserved())
                .expect("single sequence explicit indentation");
        }
    }

    #[test]
    fn accepts_plain_flow_scalar_continuation_after_a_blank_line() {
        let source = concat!(
            "proxies: [{type: vendor-private, password: hello\n",
            "\n",
            "    &literal text}]\n",
        );
        let preview = preview_text(source, "", &empty_reserved()).expect("flow scalar text");
        assert_eq!(preview.counts.added, 1);
    }

    #[test]
    fn rejects_excessive_yaml_blank_line_run_before_deserialization() {
        let source = format!(
            "proxies:\n{}  - type: vendor-private\n",
            "  \n".repeat(super::MAX_YAML_NESTING_DEPTH + 1)
        );
        let error = preview_text(&source, "", &empty_reserved()).expect_err("blank line bound");
        assert!(matches!(error, super::ImportError::Invalid(_)));
    }

    #[test]
    fn rejects_excessive_cr_only_blank_line_run_before_deserialization() {
        let source = format!(
            "proxies:\r{}  - type: vendor-private\r",
            "  \r".repeat(super::MAX_YAML_NESTING_DEPTH + 1)
        );
        let error = preview_text(&source, "", &empty_reserved()).expect_err("CR blank line bound");
        assert!(matches!(error, super::ImportError::Invalid(_)));
    }

    #[test]
    fn accepts_crlf_blank_lines_within_the_preparse_limit() {
        let source = format!(
            "proxies:\r{}  - type: vendor-private\r",
            "  \r\n".repeat(super::MAX_YAML_NESTING_DEPTH / 2 + 1)
        );
        let preview = preview_text(&source, "", &empty_reserved()).expect("CRLF blank lines");
        assert_eq!(preview.counts.added, 1);
    }

    #[test]
    fn rejects_nested_flow_after_a_plain_scalar_pipe() {
        let depth = super::MAX_YAML_NESTING_DEPTH + 1;
        let source = format!(
            "proxies: [{{type: vendor-private, note: hello |literal, value: {}{}}}]\n",
            "[".repeat(depth),
            "]".repeat(depth)
        );
        let error = preview_text(&source, "", &empty_reserved()).expect_err("flow depth bound");
        assert!(matches!(error, super::ImportError::Invalid(_)));
    }

    #[test]
    fn rejects_aliases_after_a_block_scalar() {
        let error = preview_text(
            "proxies:\n  - type: vendor-private\n    note: |\n      safe &literal\n    value: *literal\n",
            "",
            &empty_reserved(),
        )
        .expect_err("alias after block scalar");
        assert!(matches!(error, super::ImportError::Invalid(_)));
    }

    #[test]
    fn ignores_anchor_like_block_scalar_lines_for_preparse_limits() {
        let mut source = String::from("proxies:\n  - type: vendor-private\n    note: |\n");
        for _ in 0..=super::MAX_YAML_NESTING_DEPTH {
            source.push_str("      &literal\n");
        }
        let preview =
            preview_text(&source, "", &empty_reserved()).expect("block scalar marker text");
        assert_eq!(preview.counts.added, 1);
    }

    #[test]
    fn accepts_shallow_yaml_with_wide_indentation() {
        let indent = " ".repeat(130);
        let child_indent = " ".repeat(132);
        let source = format!(
            "proxies:\n{indent}- type: vendor-private\n{child_indent}server: example.com\n{child_indent}port: 443\n"
        );
        let preview = preview_text(&source, "", &empty_reserved()).expect("wide indentation");
        assert_eq!(preview.counts.added, 1);
    }

    #[test]
    fn imports_url_safe_unpadded_base64_and_yaml() {
        let yaml = r#"mixed-port: 7890
proxy-groups:
  - name: ignored
proxies:
  - type: tuic
    server: example.com
    port: 443
    token: example-token
"#;
        let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(yaml);
        let preview = preview_text(&encoded, "", &empty_reserved()).expect("preview");
        assert_eq!(preview.detected_source, DetectedSource::Base64);
        assert_eq!(preview.counts.added, 1);
        assert!(
            preview.items[0]
                .warnings
                .iter()
                .any(|warning| warning == "ignored_root_fields")
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

    #[test]
    fn rejects_yaml_aliases_before_expansion_for_source_and_draft() {
        let source_error = preview_text(
            r#"proxies:
  - &shared
    type: vendor-private
    server: example.com
    port: 443
  - *shared
"#,
            "",
            &empty_reserved(),
        )
        .expect_err("source aliases must be rejected");
        assert!(matches!(source_error, super::ImportError::Invalid(_)));

        let draft_error = preview_text(
            "vless://00000000-0000-0000-0000-000000000001@example.com:443",
            r#"- &shared
  name: existing
  type: vendor-private
  server: example.com
  port: 443
"#,
            &empty_reserved(),
        )
        .expect_err("draft aliases must be rejected");
        assert!(matches!(draft_error, super::ImportError::Invalid(_)));

        let bypass_yaml = r#"proxies:
  - type: vendor-private
    server: example.com
    port: 443
    a"b: &shared bounded
    note: x#y &other
    quoted: 'x''y' &third
  - type: vendor-private
    server: example.com
    port: 444
    a"b: *shared
"#;
        let encoded = base64::engine::general_purpose::STANDARD.encode(bypass_yaml);
        let encoded_error = preview_text(&encoded, "", &empty_reserved())
            .expect_err("encoded YAML aliases must be rejected before probing");
        assert!(matches!(encoded_error, super::ImportError::Invalid(_)));

        for bypass_yaml in [
            r#"proxies:
  - type: vendor-private
    a "b: &shared bounded-sample
  - type: vendor-private
    a "b: *shared
"#,
            "proxies: [{name: a, type: vendor-private, note: \"hello\n  \", value: &shared 'payload'}, {name: b, type: vendor-private, value: *shared}]\n",
            r#"proxies:
  - type: vendor-private
    note: hello
      "quoted
    value: &shared payload
  - type: vendor-private
    value: *shared
"#,
            r#"ignored:
  - text: |
      harmless
    a: &shared [bounded, sample]
    b: [*shared, *shared]
proxies: []
"#,
            r#"ignored:
  - - text: |
        harmless
      a: &shared [bounded, sample]
      b: [*shared, *shared]
proxies: []
"#,
            r#"ignored:
  - - |
        harmless
      - &shared payload
      - *shared
proxies: []
"#,
            r#"- &shared [bounded, sample]
- *shared
"#,
            r#"proxies: [{type: vendor-private}, &shared {type: vendor-private, extra: x}, *shared]
"#,
            r#"&root [vendor-private, &shared payload, *shared]
"#,
            r#"# comment
&root [vendor-private, &shared payload, *shared]
"#,
            r#"proxies: [{type: vendor-private, value: x:"quote}, &shared {type: vendor-private}, *shared]
"#,
            r#"# comment
[&shared payload, *shared]
"#,
            "proxies:\r  - type: vendor-private\r    value: &shared payload\r  - type: vendor-private\r    value: *shared\r",
            "\u{feff}&root [vendor-private, &shared payload, *shared]\n",
        ] {
            let error = preview_text(bypass_yaml, "", &empty_reserved())
                .err()
                .unwrap_or_else(|| panic!("YAML alias bypass was accepted"));
            assert!(matches!(error, super::ImportError::Invalid(_)));
        }

        let sibling_key_error = preview_text(
            r#"proxies:
  - type: vendor-private
    server: example.com
    port: 443
    note: plain
    &shared key: value
    [*shared]: value
"#,
            "",
            &empty_reserved(),
        )
        .expect_err("same-level YAML key aliases must be rejected");
        assert!(matches!(sibling_key_error, super::ImportError::Invalid(_)));

        let nested_sequence_error = preview_text(
            r#"ignored:
  - - |2
      safe &literal text
    - &shared value
    - *shared
proxies: []
"#,
            "",
            &empty_reserved(),
        )
        .expect_err("nested compact sequence aliases must be rejected");
        assert!(matches!(
            nested_sequence_error,
            super::ImportError::Invalid(_)
        ));

        for input in [
            "proxies:\u{0085}  - type: vendor-private\u{0085}    note: plain\u{0085}    &shared key: value\u{0085}    [*shared]: value\u{0085}",
            "- &shared\u{0085}  name: existing\u{0085}  type: vendor-private\u{0085}  server: example.com\u{0085}  port: 443\u{0085}",
        ] {
            let error = preview_text(
                "vless://00000000-0000-0000-0000-000000000001@example.com:443",
                input,
                &empty_reserved(),
            )
            .err()
            .unwrap_or_else(|| panic!("Unicode line separator was accepted: {input:?}"));
            assert!(matches!(error, super::ImportError::Invalid(_)));
        }

        for scalar in ["enabled: true", "port: 443", "ratio: 1.5", "empty: null"] {
            let source = format!(
                "proxies:\n  - type: vendor-private\n    server: example.com\n    port: 443\n    {scalar}\n    &shared key: value\n"
            );
            let error = preview_text(&source, "", &empty_reserved())
                .err()
                .unwrap_or_else(|| panic!("anchor after {scalar} was accepted"));
            assert!(matches!(error, super::ImportError::Invalid(_)));
        }

        let safe_plain_scalars = preview_text(
            r#"proxies:
  - type: vendor-private
    server: example.com
    url: https://example.com/?a=1&b=2
    note: x#y *literal
    password: secret, *literal
    description: |
      safe &literal *reference
"#,
            "",
            &empty_reserved(),
        )
        .expect("plain scalar ampersands are not aliases");
        assert_eq!(safe_plain_scalars.counts.added, 1);
    }

    #[test]
    fn rejects_non_appendable_flow_and_document_end_drafts() {
        let source = "vless://00000000-0000-0000-0000-000000000001@example.com:443";
        for existing in [
            "[]",
            "- name: existing\n  type: ss\n  server: example.com\n  port: 443\n  cipher: aes-256-gcm\n  password: secret\n...\n",
        ] {
            let error = preview_text(source, existing, &empty_reserved())
                .expect_err("append must preserve a valid YAML sequence");
            assert!(matches!(error, super::ImportError::Invalid(_)));
        }
    }

    #[test]
    fn detects_quoted_yaml_after_base64_decoding() {
        let yaml = r#"'proxies':
  - type: ss
    server: example.com
    port: 443
    cipher: aes-256-gcm
    password: secret
"#;
        let encoded = base64::engine::general_purpose::STANDARD.encode(yaml);
        let preview = preview_text(&encoded, "", &empty_reserved()).expect("preview");
        assert_eq!(preview.detected_source, DetectedSource::Base64);
        assert_eq!(preview.counts.added, 1);
    }

    #[test]
    fn preserves_standard_hysteria2_auth_pin_and_default_port() {
        let preview = preview_text(
            "hysteria2://user:secret@hy.example?pinSHA256=sha256-pin#hy",
            "",
            &empty_reserved(),
        )
        .expect("preview");
        assert_eq!(preview.counts.added, 1);
        assert!(preview.added_proxies_yaml.contains("password: user:secret"));
        assert!(preview.added_proxies_yaml.contains("port: 443"));
        assert!(
            preview
                .added_proxies_yaml
                .contains("fingerprint: sha256-pin")
        );

        for (uri, expected_password) in [
            ("hysteria2://name:@hy.example:443", "name:"),
            ("hysteria2://:pass@hy.example:443", ":pass"),
        ] {
            let preview = preview_text(uri, "", &empty_reserved()).expect("hysteria2 auth");
            let proxies: serde_yaml::Value =
                serde_yaml::from_str(&preview.added_proxies_yaml).expect("serialized proxy");
            assert!(
                proxies[0]["password"].as_str() == Some(expected_password),
                "{uri} did not preserve its authentication delimiter: {proxies:?}"
            );
        }
    }

    #[test]
    fn preserves_vmess_opaque_payload_and_alter_id() {
        let payload = (0..256)
            .map(|index| {
                format!(
                    r#"{{"v":"2","ps":"{}?","add":"vmess.example","port":"443","id":"00000000-0000-0000-0000-000000000001","aid":"3","net":"tcp"}}"#,
                    "a".repeat(index)
                )
            })
            .find(|payload| {
                base64::engine::general_purpose::STANDARD
                    .encode(payload)
                    .contains('/')
            })
            .expect("a VMess payload with a slash");
        let encoded = base64::engine::general_purpose::STANDARD.encode(payload);
        assert!(encoded.contains('/'));
        let preview =
            preview_text(&format!("vmess://{encoded}"), "", &empty_reserved()).expect("preview");
        assert_eq!(preview.counts.added, 1);
        assert!(preview.added_proxies_yaml.contains("alterId: 3"));
    }

    #[test]
    fn preserves_ipv6_and_explicit_ss_port_80() {
        let ss_payload =
            base64::engine::general_purpose::STANDARD.encode("aes-256-gcm:secret@example.com:80");
        let source = format!(
            "vless://{IMPORT_UUID}@[2001:db8::1]:443#ipv6\nss://{ss_payload}#ss",
            IMPORT_UUID = "00000000-0000-0000-0000-000000000004",
            ss_payload = ss_payload,
        );
        let preview = preview_text(&source, "", &empty_reserved()).expect("preview");
        assert_eq!(preview.counts.added, 2, "{:?}", preview.items);
        assert!(preview.added_proxies_yaml.contains("server: 2001:db8::1"));
        assert!(preview.added_proxies_yaml.contains("port: 80"));
    }

    #[test]
    fn preserves_ss_opaque_payload_and_rejects_empty_or_missing_parts() {
        let encoded =
            base64::engine::general_purpose::STANDARD.encode("aes-256-gcm:aa?@example.com:443");
        assert!(encoded.contains('/'));
        let preview = preview_text(&format!("ss://{encoded}#opaque"), "", &empty_reserved())
            .expect("opaque SS payload");
        assert_eq!(preview.counts.added, 1);
        assert!(preview.added_proxies_yaml.contains("password: aa?"));

        for payload in [
            "aes-256-gcm:@example.com:443",
            "aes-256-gcm:secret@example.com",
        ] {
            let encoded = base64::engine::general_purpose::STANDARD.encode(payload);
            let preview =
                preview_text(&format!("ss://{encoded}"), "", &empty_reserved()).expect("preview");
            assert_eq!(preview.counts.failed, 1);
        }
    }

    #[test]
    fn deduplication_preserves_typed_yaml_keys() {
        let preview = preview_text(
            r#"proxies:
  - name: first
    type: vendor-private
    server: example.com
    port: 443
    ? [1, 2]
    : one
  - name: second
    type: vendor-private
    server: example.com
    port: 443
    ? [1, 2]
    : two
"#,
            "",
            &empty_reserved(),
        )
        .expect("preview");
        assert_eq!(preview.counts.added, 2);
        assert!(preview.added_proxies_yaml.contains("first"));
        assert!(preview.added_proxies_yaml.contains("second"));
    }

    #[test]
    fn deduplication_distinguishes_mapping_sequence_and_nan_values() {
        let preview = preview_text(
            r#"proxies:
  - name: mapping
    type: vendor-private
    server: example.com
    port: 443
    options:
      mode: value
    marker: .nan
  - name: sequence
    type: vendor-private
    server: example.com
    port: 443
    options:
      - [mode, value]
    marker: null
"#,
            "",
            &empty_reserved(),
        )
        .expect("preview");
        assert_eq!(preview.counts.added, 2);
    }

    include!("mihomo_import_extra_tests.rs");
}
