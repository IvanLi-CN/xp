use axum::{
    Json,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::Serialize;
use serde_json::{Map, Value, json};

use super::ApiError;

impl ApiError {
    pub fn with_detail<T: Serialize>(mut self, key: impl Into<String>, value: T) -> Self {
        self.details.insert(
            key.into(),
            serde_json::to_value(value).unwrap_or(Value::Null),
        );
        self
    }

    pub fn with_retry_after_seconds(mut self, seconds: u64) -> Self {
        let seconds = seconds.clamp(1, 300);
        self.details
            .insert("retry_after_seconds".to_string(), json!(seconds));
        self
    }
}

#[derive(Serialize)]
struct ErrorResponse {
    error: ErrorBody,
}

#[derive(Serialize)]
struct ErrorBody {
    code: String,
    message: String,
    details: Map<String, Value>,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let retry_after = self
            .details
            .get("retry_after_seconds")
            .and_then(Value::as_u64)
            .or((self.status == StatusCode::TOO_MANY_REQUESTS).then_some(1));
        let body = ErrorResponse {
            error: ErrorBody {
                code: self.code.to_string(),
                message: self.message,
                details: self.details,
            },
        };
        let mut response = (self.status, Json(body)).into_response();
        if let Some(retry_after) = retry_after {
            response.headers_mut().insert(
                header::RETRY_AFTER,
                retry_after.to_string().parse().expect("valid header"),
            );
        }
        response
    }
}
