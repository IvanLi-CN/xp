use crate::mihomo_redact::{
    RedactError, RedactionLevel, SourceFormat, UrlLoadPolicy, load_text_from_url,
    redact_loaded_text,
};
use crate::ops::cli::{
    ExitError, MihomoRedactArgs, MihomoRedactionLevelArg, MihomoSourceFormatArg,
};
use crate::ops::paths::Paths;
use std::fs;
use std::io::Read;

impl From<MihomoRedactionLevelArg> for RedactionLevel {
    fn from(value: MihomoRedactionLevelArg) -> Self {
        match value {
            MihomoRedactionLevelArg::Minimal => RedactionLevel::Minimal,
            MihomoRedactionLevelArg::Credentials => RedactionLevel::Credentials,
            MihomoRedactionLevelArg::CredentialsAndAddress => RedactionLevel::CredentialsAndAddress,
        }
    }
}

impl From<MihomoSourceFormatArg> for SourceFormat {
    fn from(value: MihomoSourceFormatArg) -> Self {
        match value {
            MihomoSourceFormatArg::Auto => SourceFormat::Auto,
            MihomoSourceFormatArg::Raw => SourceFormat::Raw,
            MihomoSourceFormatArg::Base64 => SourceFormat::Base64,
            MihomoSourceFormatArg::Yaml => SourceFormat::Yaml,
        }
    }
}

impl From<RedactError> for ExitError {
    fn from(value: RedactError) -> Self {
        ExitError::new(value.code, value.message)
    }
}

pub async fn cmd_mihomo_redact(_paths: Paths, args: MihomoRedactArgs) -> Result<(), ExitError> {
    let raw = load_source(&args).await?;
    let redacted = redact_loaded_text(
        &raw,
        SourceFormat::from(args.source_format),
        RedactionLevel::from(args.level),
    )?;

    print!("{redacted}");
    Ok(())
}

async fn load_source(args: &MihomoRedactArgs) -> Result<String, ExitError> {
    if let Some(source) = &args.source {
        if source == "-" {
            return read_stdin_source();
        }
        if is_http_url(source) {
            return load_text_from_url(source, args.timeout_secs.max(1), UrlLoadPolicy::AllowAny)
                .await
                .map_err(ExitError::from);
        }
        return fs::read_to_string(source)
            .map_err(|e| ExitError::new(4, format!("io_error: read source file: {e}")));
    }

    read_stdin_source()
}

fn read_stdin_source() -> Result<String, ExitError> {
    let mut input = String::new();
    std::io::stdin()
        .read_to_string(&mut input)
        .map_err(|e| ExitError::new(4, format!("io_error: read stdin: {e}")))?;
    Ok(input)
}

fn is_http_url(source: &str) -> bool {
    source.starts_with("http://") || source.starts_with("https://")
}
