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
        blackjack: two_seven::blackjack::BlackjackStore::new(),
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
async fn card_test_renders_full_deck_with_game_card_faces() {
    let t = appx().await;
    let r = t
        .router
        .oneshot(
            Request::builder()
                .uri("/card-test")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let b = to_bytes(r.into_body(), usize::MAX).await.unwrap();
    let body = String::from_utf8_lossy(&b);
    assert_eq!(body.matches("playing-card").count(), 52);
    assert!(body.contains("card-art-A"));
    assert!(body.contains("pip-grid-10"));
    assert!(body.contains("card-art-K"));
    assert!(body.contains(">10<"));
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
async fn game_setup_offers_six_presets() {
    let t = appx().await;
    let response = t
        .router
        .oneshot(
            Request::builder()
                .uri("/tables/new")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let html = String::from_utf8_lossy(&body);
    assert_eq!(html.matches("class=\"setup-option\"").count(), 6);
    assert!(html.contains("quick-game-form"));
}

#[tokio::test]
async fn game_entries_enforce_one_dollar_floor_and_ten_thousand_dollar_ceiling() {
    let t = appx().await;
    let user = Uuid::new_v4();
    t.users
        .insert(User {
            id: user,
            username: "limits".into(),
            display_name: "Limits".into(),
            credentials: vec![],
            settings: UserSettings::default(),
            created_at: chrono::Utc::now(),
        })
        .await
        .unwrap();
    let cookie_value = cookie(&t.key, user);

    let small_table = t
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/tables")
                .header(header::COOKIE, &cookie_value)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"name":"Tiny","stakes":{"NoLimit":{"small_blind":99,"big_blind":200}},"min_buy_in":100,"max_buy_in":10000}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(small_table.status(), StatusCode::BAD_REQUEST);

    let huge_tournament = t
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/tournaments")
                .header(header::COOKIE, &cookie_value)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"name":"Huge","buy_in":1000001,"seat_count":4,"starting_chips":10000,"levels":[{"small_blind":100,"big_blind":200,"ante":0,"hands":10}],"payout_percentages":[100]}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(huge_tournament.status(), StatusCode::BAD_REQUEST);

    for bet in [99, 1_000_001] {
        let blackjack = t
            .router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/blackjack/start")
                    .header(header::COOKIE, &cookie_value)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(format!(r#"{{"bet":{bet}}}"#)))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(blackjack.status(), StatusCode::BAD_REQUEST);
    }
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
        matches!(entry.kind, LedgerKind::HandBlitzBuyIn { .. }) && entry.delta == -1000
    }));
    assert!(account.entries.iter().any(|entry| {
        matches!(entry.kind, LedgerKind::HandBlitzWin { .. }) && entry.delta == 333
    }));
}

#[tokio::test]
async fn blackjack_start_charges_bet_and_stand_finishes_game() {
    let t = appx().await;
    let id = Uuid::new_v4();
    t.users
        .insert(User {
            id,
            username: "blackjack".into(),
            display_name: "Blackjack".into(),
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
                .uri("/blackjack/start")
                .header(header::COOKIE, &cookie_value)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"bet":2500}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(start.status(), StatusCode::OK);
    let body: serde_json::Value =
        serde_json::from_slice(&to_bytes(start.into_body(), usize::MAX).await.unwrap()).unwrap();
    let game_id = body["id"].as_str().unwrap();
    if body["can_stand"].as_bool().unwrap() {
        let stand = t
            .router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/blackjack/stand")
                    .header(header::COOKIE, &cookie_value)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(format!(r#"{{"id":"{game_id}"}}"#)))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(stand.status(), StatusCode::OK);
        let body: serde_json::Value =
            serde_json::from_slice(&to_bytes(stand.into_body(), usize::MAX).await.unwrap())
                .unwrap();
        assert!(!body["can_hit"].as_bool().unwrap());
        assert!(body["dealer_score"].as_u64().is_some());
    }
    let account = t.bank.account(AccountOwner::User(id)).await.unwrap();
    assert!(account.entries.iter().any(|entry| {
        matches!(entry.kind, LedgerKind::BlackjackBet { .. }) && entry.delta == -2500
    }));
    assert_eq!(
        account.entries.iter().map(|entry| entry.delta).sum::<i64>(),
        account.balance
    );
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
    let create = t.router.clone().oneshot(Request::builder().method("POST").uri("/tables").header(header::COOKIE, &cookie_a).header(header::CONTENT_TYPE, "application/json").body(Body::from(r#"{"name":"Test","stakes":{"NoLimit":{"small_blind":100,"big_blind":200}},"max_seats":2,"min_buy_in":1000,"max_buy_in":10000}"#)).unwrap()).await.unwrap();
    assert_eq!(create.status(), StatusCode::OK);
    let body = to_bytes(create.into_body(), usize::MAX).await.unwrap();
    let id: Uuid = serde_json::from_slice::<serde_json::Value>(&body).unwrap()["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    for (cookie_value, seat) in [(&cookie_a, 0), (&cookie_b, 1)] {
        let request = format!(r#"{{"seat":{seat},"buy_in":2000}}"#);
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
    assert!(text.contains("street_contribution"));
    assert!(text.contains("SmallBlind"));
    assert!(text.contains(r#""hole_cards":null"#));
    let wrong_turn = t
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/tables/{id}/action"))
                .header(header::COOKIE, &cookie_b)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"kind":"check"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(wrong_turn.status(), StatusCode::BAD_REQUEST);
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
    assert_eq!(account["balance"], -100);
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
                    r#"{"name":"No debt bots","stakes":{"NoLimit":{"small_blind":100,"big_blind":200}},"no_debt":true,"min_buy_in":1000,"max_buy_in":10000}"#,
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
    let create = t.router.clone().oneshot(Request::builder().method("POST").uri("/tables").header(header::COOKIE, &cookie_value).header(header::CONTENT_TYPE, "application/json").body(Body::from(r#"{"name":"No debt","stakes":{"NoLimit":{"small_blind":100,"big_blind":200}},"no_debt":true,"min_buy_in":1000,"max_buy_in":10000}"#)).unwrap()).await.unwrap();
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
                .body(Body::from(r#"{"seat":0,"buy_in":2000}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}
