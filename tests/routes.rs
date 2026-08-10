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
    bank::{AccountOwner, BankStore, LedgerKind},
    cards::Card,
    eval::evaluate,
    users::{User, UserSettings, UserStore},
};
use uuid::Uuid;
struct T {
    router: Router,
    key: Key,
    users: Arc<UserStore>,
    bank: BankStore,
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
    let blitz = two_seven::blitz::BlitzStore::load(&dir).await.unwrap();
    let tables = two_seven::store::TableStore::load(&dir).await.unwrap();
    let key = Key::generate();
    let state = app::AppState {
        users: users.clone(),
        bank: bank.clone(),
        blitz,
        tables,
        webauthn: Arc::new(app::build_webauthn().unwrap()),
        key: key.clone(),
        passkey_disabled: true,
    };
    T {
        router: app::router(state),
        key,
        users,
        bank,
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
async fn hand_blitz_start_charges_buy_in_and_correct_answer_pays() {
    let t = appx().await;
    let id = Uuid::new_v4();
    t.users
        .insert(User {
            id,
            username: "blitz".into(),
            display_name: "Blitz".into(),
            credentials: vec![],
            settings: UserSettings::default(),
            created_at: chrono::Utc::now(),
        })
        .await
        .unwrap();
    let cookie_value = cookie(&t.key, id);
    let start = t
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/hand-blitz/start")
                .header(header::COOKIE, &cookie_value)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"difficulty":"easy"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(start.status(), StatusCode::OK);
    let body: serde_json::Value =
        serde_json::from_slice(&to_bytes(start.into_body(), usize::MAX).await.unwrap()).unwrap();
    let run_id = body["run"]["id"].as_str().unwrap();
    let round_id = body["run"]["round"]["id"].as_str().unwrap();

    let board = body["run"]["round"]["board"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap().parse::<Card>().unwrap())
        .collect::<Vec<_>>();
    let hands = body["run"]["round"]["hands"].as_array().unwrap();
    let ranks = [0usize, 1].map(|index| {
        let mut cards = hands[index]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap().parse::<Card>().unwrap())
            .collect::<Vec<_>>();
        cards.extend(board.clone());
        evaluate(&cards).rank
    });
    let winner = usize::from(ranks[1] > ranks[0]);
    let response = t
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/hand-blitz/answer")
                .header(header::COOKIE, &cookie_value)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(format!(
                    r#"{{"run_id":"{run_id}","round_id":"{round_id}","choice":{winner}}}"#
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let answer: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert!(answer["correct"].as_bool().unwrap());
    let account = t.bank.account(AccountOwner::User(id)).await.unwrap();
    assert!(account.entries.iter().any(|entry| {
        matches!(entry.kind, LedgerKind::HandBlitzBuyIn { .. }) && entry.delta == -100
    }));
    assert!(account.entries.iter().any(|entry| {
        matches!(entry.kind, LedgerKind::HandBlitzWin { .. }) && entry.delta == 33
    }));
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
    let replace_human = t
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/tables/{id}/bot"))
                .header(header::COOKIE, &cookie_a)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"seat":0,"kind":"fish"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(replace_human.status(), StatusCode::BAD_REQUEST);
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
async fn no_debt_cash_bot_rebuy_is_rejected() {
    let t = appx().await;
    let user = Uuid::new_v4();
    t.users
        .insert(User {
            id: user,
            username: "bot-owner".into(),
            display_name: "Bot Owner".into(),
            credentials: vec![],
            settings: UserSettings::default(),
            created_at: chrono::Utc::now(),
        })
        .await
        .unwrap();
    let cookie_value = cookie(&t.key, user);
    let create = t
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/tables")
                .header(header::COOKIE, &cookie_value)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"name":"No debt bots","stakes":{"NoLimit":{"small_blind":1,"big_blind":2}},"no_debt":true,"min_buy_in":10,"max_buy_in":100}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
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
                .uri(format!("/tables/{id}/bot"))
                .header(header::COOKIE, cookie_value)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"seat":0,"kind":"fish"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert!(String::from_utf8_lossy(&body).contains("insufficient funds"));
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
