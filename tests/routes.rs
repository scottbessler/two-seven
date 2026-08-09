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
    let bank = two_seven::bank::BankStore::load(&dir).await.unwrap();
    let tables = two_seven::store::TableStore::load(&dir).await.unwrap();
    let key = Key::generate();
    let state = app::AppState {
        users: users.clone(),
        bank,
        tables,
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

#[tokio::test]
async fn table_join_starts_hand_and_redacts_opponent_cards() {
    let t = appx().await;
    let alice = Uuid::new_v4();
    let bob = Uuid::new_v4();
    for (id, name) in [(alice, "Alice"), (bob, "Bob")] {
        t.users
            .insert(User {
                id,
                username: name.into(),
                display_name: name.into(),
                credentials: vec![],
                settings: UserSettings::default(),
                created_at: chrono::Utc::now(),
            })
            .await
            .unwrap();
    }
    let cookie_a = cookie(&t.key, alice);
    let cookie_b = cookie(&t.key, bob);
    let create = t.router.clone().oneshot(Request::builder().method("POST").uri("/tables").header(header::COOKIE, &cookie_a).header(header::CONTENT_TYPE, "application/json").body(Body::from(r#"{"name":"Test","stakes":{"NoLimit":{"small_blind":1,"big_blind":2}},"max_seats":2,"min_buy_in":10,"max_buy_in":100}"#)).unwrap()).await.unwrap();
    assert_eq!(create.status(), StatusCode::OK);
    let body = to_bytes(create.into_body(), usize::MAX).await.unwrap();
    let id: Uuid = serde_json::from_slice::<serde_json::Value>(&body).unwrap()["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    for (cookie_value, seat) in [(&cookie_a, 0), (&cookie_b, 1)] {
        let request = format!(r#"{{"seat":{seat},"buy_in":20}}"#);
        let response = t
            .router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/tables/{id}/join"))
                    .header(header::COOKIE, cookie_value)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(request))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
    let state = t
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/tables/{id}/state"))
                .header(header::COOKIE, &cookie_a)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(state.status(), StatusCode::OK);
    let text = String::from_utf8(
        to_bytes(state.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(text.contains("current_player"));
    assert!(text.contains(r#""hole_cards":null"#));
    let fold = t
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/tables/{id}/action"))
                .header(header::COOKIE, &cookie_a)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"kind":"fold"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(fold.status(), StatusCode::OK);
    for cookie_value in [&cookie_a, &cookie_b] {
        let leave = t
            .router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/tables/{id}/leave"))
                    .header(header::COOKIE, cookie_value)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(leave.status(), StatusCode::OK);
    }
    let account = t
        .router
        .oneshot(
            Request::builder()
                .uri("/api/bank")
                .header(header::COOKIE, &cookie_a)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let account: serde_json::Value =
        serde_json::from_slice(&to_bytes(account.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(account["balance"], -1);
    assert_eq!(account["entries"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn no_debt_join_is_rejected() {
    let t = appx().await;
    let user = Uuid::new_v4();
    t.users
        .insert(User {
            id: user,
            username: "NoDebt".into(),
            display_name: "NoDebt".into(),
            credentials: vec![],
            settings: UserSettings::default(),
            created_at: chrono::Utc::now(),
        })
        .await
        .unwrap();
    let cookie_value = cookie(&t.key, user);
    let create = t.router.clone().oneshot(Request::builder().method("POST").uri("/tables").header(header::COOKIE, &cookie_value).header(header::CONTENT_TYPE, "application/json").body(Body::from(r#"{"name":"No debt","stakes":{"NoLimit":{"small_blind":1,"big_blind":2}},"no_debt":true,"min_buy_in":10,"max_buy_in":100}"#)).unwrap()).await.unwrap();
    let id: Uuid = serde_json::from_slice::<serde_json::Value>(
        &to_bytes(create.into_body(), usize::MAX).await.unwrap(),
    )
    .unwrap()["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let response = t
        .router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/tables/{id}/join"))
                .header(header::COOKIE, cookie_value)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"seat":0,"buy_in":20}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}
