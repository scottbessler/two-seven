use crate::error::AppError;
use axum::{
    extract::FromRef,
    http::{StatusCode, request::Parts},
    response::{IntoResponse, Response},
};
use axum_extra::extract::cookie::{Cookie, Key, SameSite, SignedCookieJar};
use serde_json::json;
use std::convert::Infallible;
use uuid::Uuid;
pub const SESSION_COOKIE: &str = "sid";
pub fn session_cookie(id: Uuid) -> Cookie<'static> {
    Cookie::build((SESSION_COOKIE, id.to_string()))
        .path("/")
        .http_only(true)
        .same_site(SameSite::Lax)
        .permanent()
        .build()
}
fn read(j: &SignedCookieJar) -> Option<Uuid> {
    j.get(SESSION_COOKIE).and_then(|c| c.value().parse().ok())
}
#[derive(Clone, Copy, Debug)]
pub struct AuthUser(pub Uuid);
#[derive(Clone, Copy, Debug)]
pub struct MaybeUser(pub Option<Uuid>);
#[derive(Clone, Copy, Debug)]
pub struct ApiAuthUser(pub Uuid);
impl<S> axum::extract::FromRequestParts<S> for MaybeUser
where
    S: Send + Sync,
    Key: FromRef<S>,
{
    type Rejection = Infallible;
    async fn from_request_parts(p: &mut Parts, s: &S) -> Result<Self, Self::Rejection> {
        let j = SignedCookieJar::<Key>::from_request_parts(p, s)
            .await
            .expect("cookie");
        Ok(Self(read(&j)))
    }
}
impl<S> axum::extract::FromRequestParts<S> for AuthUser
where
    S: Send + Sync,
    Key: FromRef<S>,
{
    type Rejection = AppError;
    async fn from_request_parts(p: &mut Parts, s: &S) -> Result<Self, Self::Rejection> {
        let j = SignedCookieJar::<Key>::from_request_parts(p, s)
            .await
            .expect("cookie");
        read(&j)
            .map(Self)
            .ok_or_else(|| AppError::unauthorized("sign in to do that"))
    }
}
impl<S> axum::extract::FromRequestParts<S> for ApiAuthUser
where
    S: Send + Sync,
    Key: FromRef<S>,
{
    type Rejection = Response;
    async fn from_request_parts(p: &mut Parts, s: &S) -> Result<Self, Self::Rejection> {
        let j = SignedCookieJar::<Key>::from_request_parts(p, s)
            .await
            .expect("cookie");
        read(&j).map(Self).ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                axum::Json(json!({"error":"sign in to do that"})),
            )
                .into_response()
        })
    }
}
