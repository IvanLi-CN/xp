use super::{RedactError, RedactErrorKind};

pub(super) fn target_error(error: RedactError, redirect_count: usize) -> RedactError {
    if redirect_count > 0 && error.kind == RedactErrorKind::InvalidInput {
        RedactError::network("network_error: redirect target was rejected")
    } else {
        error
    }
}
