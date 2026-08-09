use crate::{
    app::AppState,
    error::AppError,
    session::{SESSION_COOKIE, session_cookie},
    users::{User, UserSettings},
};
use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Redirect, Response},
};
use axum_extra::extract::cookie::{Cookie, SameSite, SignedCookieJar};
use chrono::Utc;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::json;
use uuid::Uuid;
use webauthn_rs::prelude::{
    PasskeyAuthentication, PasskeyRegistration, PublicKeyCredential, RegisterPublicKeyCredential,
};
const REG_COOKIE: &str = "reg_state";
const AUTH_COOKIE: &str = "auth_state";
pub struct AuthReject {
    status: StatusCode,
    message: String,
}
impl AuthReject {
    fn bad(x: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: x.into(),
        }
    }
}
impl IntoResponse for AuthReject {
    fn into_response(self) -> Response {
        (self.status, Json(json!({"error":self.message}))).into_response()
    }
}
impl From<AppError> for AuthReject {
    fn from(e: AppError) -> Self {
        Self {
            status: e.status_code(),
            message: e.detail().to_string(),
        }
    }
}
#[derive(Deserialize)]
pub struct RegisterBegin {
    username: String,
    display_name: Option<String>,
}
#[derive(Serialize, Deserialize)]
struct PendingRegistration {
    user_id: Uuid,
    username: String,
    display_name: String,
    state: PasskeyRegistration,
}
#[derive(Deserialize)]
pub struct LoginBegin {
    username: String,
}
#[derive(Serialize, Deserialize)]
struct PendingLogin {
    user_id: Uuid,
    state: PasskeyAuthentication,
}
pub async fn register_begin(
    State(s): State<AppState>,
    j: SignedCookieJar,
    Json(b): Json<RegisterBegin>,
) -> Result<Response, AuthReject> {
    let u = b.username.trim().to_string();
    if u.is_empty() || u.chars().count() > 32 {
        return Err(AuthReject::bad("pick a username (1–32 characters)"));
    }
    if s.users.username_taken(&u).await {
        return Err(AuthReject {
            status: StatusCode::CONFLICT,
            message: "that username is already taken".into(),
        });
    }
    let d = b.display_name.as_deref().unwrap_or(&u).trim().to_string();
    let id = Uuid::new_v4();
    if s.passkey_disabled {
        let user = User {
            id,
            username: u,
            display_name: d.clone(),
            credentials: vec![],
            settings: UserSettings::default(),
            created_at: Utc::now(),
        };
        s.users.insert(user).await?;
        return Ok((j.add(session_cookie(id)), Json(json!({"display_name":d}))).into_response());
    }
    let (ch, state) = s
        .webauthn
        .start_passkey_registration(id, &u, &d, None)
        .map_err(|e| AuthReject::bad(e.to_string()))?;
    Ok((
        j.add(state_cookie(
            REG_COOKIE,
            &PendingRegistration {
                user_id: id,
                username: u,
                display_name: d,
                state,
            },
        )?),
        Json(ch),
    )
        .into_response())
}
pub async fn register_finish(
    State(s): State<AppState>,
    j: SignedCookieJar,
    Json(c): Json<RegisterPublicKeyCredential>,
) -> Result<(SignedCookieJar, Json<serde_json::Value>), AuthReject> {
    let p: PendingRegistration =
        take(&j, REG_COOKIE).ok_or_else(|| AuthReject::bad("registration session expired"))?;
    let pass = s
        .webauthn
        .finish_passkey_registration(&c, &p.state)
        .map_err(|e| AuthReject::bad(e.to_string()))?;
    let d = p.display_name.clone();
    s.users
        .insert(User {
            id: p.user_id,
            username: p.username,
            display_name: d.clone(),
            credentials: vec![pass],
            settings: UserSettings::default(),
            created_at: Utc::now(),
        })
        .await?;
    Ok((
        j.remove(Cookie::build((REG_COOKIE, "")).path("/").build())
            .add(session_cookie(p.user_id)),
        Json(json!({"display_name":d})),
    ))
}
pub async fn login_begin(
    State(s): State<AppState>,
    j: SignedCookieJar,
    Json(b): Json<LoginBegin>,
) -> Result<Response, AuthReject> {
    let u = s
        .users
        .get_by_username(&b.username)
        .await
        .ok_or_else(|| AuthReject {
            status: StatusCode::NOT_FOUND,
            message: "no account with that username".into(),
        })?;
    if s.passkey_disabled {
        return Ok((
            j.add(session_cookie(u.id)),
            Json(json!({"display_name":u.display_name})),
        )
            .into_response());
    }
    let (ch, state) = s
        .webauthn
        .start_passkey_authentication(&u.credentials)
        .map_err(|e| AuthReject::bad(e.to_string()))?;
    Ok((
        j.add(state_cookie(
            AUTH_COOKIE,
            &PendingLogin {
                user_id: u.id,
                state,
            },
        )?),
        Json(ch),
    )
        .into_response())
}
pub async fn login_finish(
    State(s): State<AppState>,
    j: SignedCookieJar,
    Json(c): Json<PublicKeyCredential>,
) -> Result<(SignedCookieJar, Json<serde_json::Value>), AuthReject> {
    let p: PendingLogin =
        take(&j, AUTH_COOKIE).ok_or_else(|| AuthReject::bad("login session expired"))?;
    let r = s
        .webauthn
        .finish_passkey_authentication(&c, &p.state)
        .map_err(|e| AuthReject::bad(e.to_string()))?;
    if r.needs_update() { /* credential persistence is added with account settings */ }
    Ok((j.add(session_cookie(p.user_id)), Json(json!({"ok":true}))))
}
pub async fn logout(j: SignedCookieJar) -> (SignedCookieJar, Redirect) {
    (
        j.remove(Cookie::build((SESSION_COOKIE, "")).path("/").build()),
        Redirect::to("/"),
    )
}
fn state_cookie<T: Serialize>(n: &'static str, v: &T) -> Result<Cookie<'static>, AuthReject> {
    Ok(Cookie::build((
        n,
        serde_json::to_string(v).map_err(|e| AuthReject::bad(e.to_string()))?,
    ))
    .path("/")
    .http_only(true)
    .same_site(SameSite::Lax)
    .build())
}
fn take<T: DeserializeOwned>(j: &SignedCookieJar, n: &str) -> Option<T> {
    j.get(n).and_then(|c| serde_json::from_str(c.value()).ok())
}
