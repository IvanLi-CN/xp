use crate::ops::cli::ExitError;

pub(super) fn validate_https_origin_no_port(origin: &str) -> Result<(), ExitError> {
    let url =
        reqwest::Url::parse(origin).map_err(|_| ExitError::new(2, "invalid_args: invalid url"))?;
    if url.scheme() != "https" {
        return Err(ExitError::new(
            2,
            "invalid_args: api-base-url must be https",
        ));
    }
    if url.path() != "/" || url.query().is_some() || url.fragment().is_some() {
        return Err(ExitError::new(
            2,
            "invalid_args: api-base-url must be an origin (no path/query)",
        ));
    }
    Ok(())
}
