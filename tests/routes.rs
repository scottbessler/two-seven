use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use axum_extra::extract::cookie::Key;
use cookie::{Cookie as RawCookie, CookieJar};
use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use tower::ServiceExt;
use two_seven::{
    app,
    users::{User, UserSettings, UserStore},
};
use uuid::Uuid;
struct T {
    router: Router,
    key: Key,
    users: Arc<UserStore>,
}
async fn appx() -> T {
    let dir = std::env::temp_dir().join(format!(
        "two-seven-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let users = Arc::new(UserStore::load(&dir).await.unwrap());
    let key = Key::generate();
    let state = app::AppState {
        users: users.clone(),
        webauthn: Arc::new(app::build_webauthn().unwrap()),
        key: key.clone(),
        passkey_disabled: true,
    };
    T {
        router: app::router(state),
        key,
        users,
    }
}
fn cookie(key: &Key, id: Uuid) -> String {
    let mut j = CookieJar::new();
    j.signed_mut(key).add(RawCookie::new("sid", id.to_string()));
    format!("sid={}", j.get("sid").unwrap().value())
}
#[tokio::test]
async fn home_signed_out() {
    let t = appx().await;
    let r = t
        .router
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let b = to_bytes(r.into_body(), usize::MAX).await.unwrap();
    assert!(String::from_utf8_lossy(&b).contains("Register"));
}
#[tokio::test]
async fn health() {
    let t = appx().await;
    let r = t
        .router
        .oneshot(
            Request::builder()
                .uri("/healthcheck")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
}
#[tokio::test]
async fn signed_home() {
    let t = appx().await;
    let id = Uuid::new_v4();
    t.users
        .insert(User {
            id,
            username: "alice".into(),
            display_name: "Alice".into(),
            credentials: vec![],
            settings: UserSettings::default(),
            created_at: chrono::Utc::now(),
        })
        .await
        .unwrap();
    let r = t
        .router
        .oneshot(
            Request::builder()
                .uri("/")
                .header(header::COOKIE, cookie(&t.key, id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let b = to_bytes(r.into_body(), usize::MAX).await.unwrap();
    assert!(String::from_utf8_lossy(&b).contains("Welcome, Alice"));
}
