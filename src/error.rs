use crate::render;
use axum::{
    http::StatusCode,
    response::{Html, IntoResponse, Response},
};
#[derive(Debug)]
pub enum AppError {
    BadRequest(String),
    NotFound(String),
    Unauthorized(String),
    Conflict(String),
    Internal(String),
}
impl AppError {
    pub fn bad_request(x: impl Into<String>) -> Self {
        Self::BadRequest(x.into())
    }
    pub fn not_found(x: impl Into<String>) -> Self {
        Self::NotFound(x.into())
    }
    pub fn unauthorized(x: impl Into<String>) -> Self {
        Self::Unauthorized(x.into())
    }
    pub fn conflict(x: impl Into<String>) -> Self {
        Self::Conflict(x.into())
    }
    pub fn internal(x: impl std::fmt::Display) -> Self {
        Self::Internal(x.to_string())
    }
    pub fn status_code(&self) -> StatusCode {
        match self {
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            Self::Conflict(_) => StatusCode::UNPROCESSABLE_ENTITY,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
    pub fn detail(&self) -> &str {
        match self {
            Self::BadRequest(x)
            | Self::NotFound(x)
            | Self::Unauthorized(x)
            | Self::Conflict(x)
            | Self::Internal(x) => x,
        }
    }
    fn title(&self) -> &'static str {
        match self {
            Self::BadRequest(_) => "Bad request",
            Self::NotFound(_) => "Not found",
            Self::Unauthorized(_) => "Sign in required",
            Self::Conflict(_) => "Invalid request",
            Self::Internal(_) => "Something went wrong",
        }
    }
}
impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.title(), self.detail())
    }
}
impl std::error::Error for AppError {}
impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        (
            self.status_code(),
            Html(render::error_page(self.title(), self.detail())),
        )
            .into_response()
    }
}
