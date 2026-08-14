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
        blackjack: two_seven::blackjack::BlackjackStore::load(&dir)
            .await
            .unwrap(),
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
    // The face is rank over suit only, matching card.js.
    assert_eq!(body.matches("card-corner").count(), 52);
    assert!(body.contains("<b>10</b><i>\u{2660}</i>"));
    assert!(body.contains("<b>A</b><i>\u{2665}</i>"));
    for dropped in ["card-art", "pip-grid", "court-piece", "card-frame"] {
        assert!(!body.contains(dropped), "card faces still render {dropped}");
    }
}

#[tokio::test]
async fn pages_map_module_imports_to_versioned_urls() {
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
    let b = to_bytes(r.into_body(), usize::MAX).await.unwrap();
    let body = String::from_utf8_lossy(&b);
    // The map has to precede the first module script or the browser ignores it.
    let map = body
        .find(r#"<script type="importmap">"#)
        .expect("import map");
    let island = body
        .find(r#"<script type="module""#)
        .expect("island script");
    assert!(map < island, "the import map must come before any module");
    for module in [
        "/public/card.js",
        "/public/card-settings.js",
        "/public/shared.js",
        "/public/vendor/htm-preact.js",
    ] {
        assert!(
            body.contains(&format!(r#""{module}":"{module}?v="#)),
            "{module} must resolve to a versioned URL"
        );
    }
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
async fn game_setup_walks_a_stepped_dialog() {
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
    // A stepped dialog: format, betting or players, buy-in, then confirm.
    assert_eq!(html.matches("class=\"setup-step\"").count(), 4);
    assert_eq!(html.matches("data-choice=\"buyIn\"").count(), 4);
    assert!(html.contains("data-choice=\"format\""));
    assert!(html.contains("data-choice=\"betting\""));
    assert!(html.contains("data-choice=\"players\""));
    assert!(html.contains("quick-game-form"));
    assert!(html.contains("id=\"game-setup\""));
}

#[tokio::test]
async fn tournament_accepts_the_full_ten_thousand_chip_ladder() {
    let t = appx().await;
    let user = Uuid::new_v4();
    t.users
        .insert(User {
            id: user,
            username: "ladder".into(),
            display_name: "Ladder".into(),
            credentials: vec![],
            settings: UserSettings::default(),
            created_at: chrono::Utc::now(),
        })
        .await
        .unwrap();
    let cookie_value = cookie(&t.key, user);
    // The T10,000 structure climbs to a 16,000-chip big blind, well past the
    // cash-game entry ceiling, and a level lasts six players' worth of hands.
    let body = r#"{"name":"Ladder","buy_in":50000,"seat_count":6,"starting_chips":1000000,"levels":[{"small_blind":10000,"big_blind":20000,"ante":0,"hands":12},{"small_blind":20000,"big_blind":40000,"ante":0,"hands":12},{"small_blind":30000,"big_blind":60000,"ante":0,"hands":12},{"small_blind":40000,"big_blind":80000,"ante":0,"hands":12},{"small_blind":50000,"big_blind":100000,"ante":0,"hands":12},{"small_blind":60000,"big_blind":120000,"ante":0,"hands":12},{"small_blind":80000,"big_blind":160000,"ante":0,"hands":12},{"small_blind":100000,"big_blind":200000,"ante":0,"hands":12},{"small_blind":150000,"big_blind":300000,"ante":0,"hands":12},{"small_blind":200000,"big_blind":400000,"ante":0,"hands":12},{"small_blind":300000,"big_blind":600000,"ante":0,"hands":12},{"small_blind":400000,"big_blind":800000,"ante":0,"hands":12},{"small_blind":500000,"big_blind":1000000,"ante":0,"hands":12},{"small_blind":600000,"big_blind":1200000,"ante":0,"hands":12},{"small_blind":800000,"big_blind":1600000,"ante":0,"hands":12}],"payout_percentages":[65,35]}"#;
    let create = t
        .router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/tournaments")
                .header(header::COOKIE, &cookie_value)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(create.status(), StatusCode::OK);
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
                    r#"{"name":"Tiny","stakes":{"NoLimit":{"small_blind":99,"big_blind":200}},"buy_in":10000}"#,
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

    for bet in [99, 2_500, 1_000_001] {
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
    let initial_entries = t
        .bank
        .account(AccountOwner::User(id))
        .await
        .unwrap()
        .entries
        .len();
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
    let resume = t
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/hand-blitz/resume")
                .header(header::COOKIE, &cookie_value)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let resumed: serde_json::Value =
        serde_json::from_slice(&to_bytes(resume.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(resumed["id"].as_str(), Some(run_id));

    let second_start = t
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
    assert_eq!(second_start.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        t.bank
            .account(AccountOwner::User(id))
            .await
            .unwrap()
            .entries
            .len(),
        initial_entries + 1
    );

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
                .body(Body::from(r#"{"bet":2000}"#))
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
        matches!(entry.kind, LedgerKind::BlackjackBet { .. }) && entry.delta == -2000
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
    let create = t.router.clone().oneshot(Request::builder().method("POST").uri("/tables").header(header::COOKIE, &cookie_a).header(header::CONTENT_TYPE, "application/json").body(Body::from(r#"{"name":"Test","stakes":{"NoLimit":{"small_blind":100,"big_blind":200}},"max_seats":2,"buy_in":10000}"#)).unwrap()).await.unwrap();
    assert_eq!(create.status(), StatusCode::OK);
    let body = to_bytes(create.into_body(), usize::MAX).await.unwrap();
    let id: Uuid = serde_json::from_slice::<serde_json::Value>(&body).unwrap()["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    for cookie_value in [&cookie_a, &cookie_b] {
        let response = t
            .router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/tables/{id}/join"))
                    .header(header::COOKIE, cookie_value)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from("{}"))
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
    let state_json: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert!(text.contains("current_player"));
    assert!(text.contains(r#""buy_in":10000"#));
    assert!(text.contains("street_contribution"));
    assert!(text.contains("SmallBlind"));
    assert!(text.contains(r#""hole_cards":null"#));
    assert!(
        state_json["seats"]
            .as_array()
            .unwrap()
            .iter()
            .all(|seat| seat["stack"] == 10_000),
        "the client-supplied $20 buy-ins must not override the fixed $100 table buy-in"
    );
    assert_eq!(state_json["viewer_seat"], 0);
    assert_eq!(state_json["viewer_leaving"], false);
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
    let leave = t
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/tables/{id}/leave"))
                .header(header::COOKIE, &cookie_a)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(leave.status(), StatusCode::OK);
    let leave: serde_json::Value =
        serde_json::from_slice(&to_bytes(leave.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(leave["pending"], false);
    let showdown = t
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
    let showdown: serde_json::Value =
        serde_json::from_slice(&to_bytes(showdown.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert!(showdown["hand"].is_null());
    assert!(showdown["last_hand"].is_object());
    assert!(showdown["next_hand_at"].is_string());
    assert!(showdown["viewer_seat"].is_null());
    assert_eq!(showdown["viewer_leaving"], false);
    let fold_deadline =
        chrono::DateTime::parse_from_rfc3339(showdown["next_hand_at"].as_str().unwrap())
            .unwrap()
            .with_timezone(&chrono::Utc);
    let fold_pause = fold_deadline - chrono::Utc::now();
    assert!(fold_pause >= chrono::Duration::seconds(2));
    assert!(fold_pause <= chrono::Duration::seconds(3));
    let continue_hand = t
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/tables/{id}/continue"))
                .header(header::COOKIE, &cookie_b)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(continue_hand.status(), StatusCode::OK);
    let continued = t
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
    let continued: serde_json::Value =
        serde_json::from_slice(&to_bytes(continued.into_body(), usize::MAX).await.unwrap())
            .unwrap();
    assert!(continued["hand"].is_null());
    let acknowledged_at =
        chrono::DateTime::parse_from_rfc3339(continued["next_hand_at"].as_str().unwrap())
            .unwrap()
            .with_timezone(&chrono::Utc);
    assert!(acknowledged_at <= chrono::Utc::now());
    let leave = t
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/tables/{id}/leave"))
                .header(header::COOKIE, &cookie_b)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(leave.status(), StatusCode::OK);
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
async fn tournament_registration_uses_configured_buy_in_and_first_open_seat() {
    let t = appx().await;
    let user = Uuid::new_v4();
    t.users
        .insert(User {
            id: user,
            username: "entrant".into(),
            display_name: "Entrant".into(),
            credentials: vec![],
            settings: UserSettings::default(),
            created_at: chrono::Utc::now(),
        })
        .await
        .unwrap();
    let cookie_value = cookie(&t.key, user);
    let create = t.router.clone().oneshot(Request::builder().method("POST").uri("/tournaments").header(header::COOKIE, &cookie_value).header(header::CONTENT_TYPE, "application/json").body(Body::from(r#"{"name":"Sit and Go","buy_in":5000,"seat_count":2,"starting_chips":20000,"levels":[{"small_blind":100,"big_blind":200,"ante":0,"hands":10}],"payout_percentages":[100]}"#)).unwrap()).await.unwrap();
    assert_eq!(create.status(), StatusCode::OK);
    let id: Uuid = serde_json::from_slice::<serde_json::Value>(
        &to_bytes(create.into_body(), usize::MAX).await.unwrap(),
    )
    .unwrap()["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let register = t
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/tournaments/{id}/register"))
                .header(header::COOKIE, &cookie_value)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(register.status(), StatusCode::OK);
    let state = t
        .router
        .oneshot(
            Request::builder()
                .uri(format!("/tables/{id}/state"))
                .header(header::COOKIE, &cookie_value)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let state: serde_json::Value =
        serde_json::from_slice(&to_bytes(state.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(state["viewer_seat"], 0);
    assert_eq!(state["seats"][0]["stack"], 20_000);
    assert_eq!(state["seats"][1]["occupant"], "empty");
    assert_eq!(
        t.bank
            .account(AccountOwner::User(user))
            .await
            .unwrap()
            .balance,
        -5_000
    );
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
                    r#"{"name":"No debt bots","stakes":{"NoLimit":{"small_blind":100,"big_blind":200}},"no_debt":true,"buy_in":10000}"#,
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
    let create = t.router.clone().oneshot(Request::builder().method("POST").uri("/tables").header(header::COOKIE, &cookie_value).header(header::CONTENT_TYPE, "application/json").body(Body::from(r#"{"name":"No debt","stakes":{"NoLimit":{"small_blind":100,"big_blind":200}},"no_debt":true,"buy_in":10000}"#)).unwrap()).await.unwrap();
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
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn blackjack_rejected_actions_leave_ledger_untouched() {
    let t = appx().await;
    let user = Uuid::new_v4();
    t.users
        .insert(User {
            id: user,
            username: "blackjack-reject".into(),
            display_name: "Blackjack Reject".into(),
            credentials: vec![],
            settings: UserSettings::default(),
            created_at: chrono::Utc::now(),
        })
        .await
        .unwrap();
    let cookie_value = cookie(&t.key, user);
    let response = t
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/blackjack/start")
                .header(header::COOKIE, &cookie_value)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"bet":500}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    let body: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    let game_id = body["id"].as_str().unwrap();
    if body["status"] == "Playing" {
        let response = t
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
        assert_eq!(response.status(), StatusCode::OK);
    }
    let before = t.bank.account(AccountOwner::User(user)).await.unwrap();
    for kind in ["double", "split", "insurance"] {
        let response = t
            .router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/blackjack/{kind}"))
                    .header(header::COOKIE, &cookie_value)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(format!(r#"{{"id":"{game_id}"}}"#)))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
    let after = t.bank.account(AccountOwner::User(user)).await.unwrap();
    assert_eq!(before.entries, after.entries);
}

#[tokio::test]
async fn blackjack_start_rejects_live_game_and_resume_returns_it() {
    let t = appx().await;
    let user = Uuid::new_v4();
    t.users
        .insert(User {
            id: user,
            username: "blackjack-resume".into(),
            display_name: "Blackjack Resume".into(),
            credentials: vec![],
            settings: UserSettings::default(),
            created_at: chrono::Utc::now(),
        })
        .await
        .unwrap();
    let cookie_value = cookie(&t.key, user);
    let mut live = None;
    for _ in 0..20 {
        let response = t
            .router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/blackjack/start")
                    .header(header::COOKIE, &cookie_value)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"bet":500}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        let body: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap())
                .unwrap();
        if body["status"] == "Playing" {
            live = Some(body);
            break;
        }
    }
    let live = live.expect("seed a live blackjack hand");
    let game_id = live["id"].as_str().unwrap();
    let before = t.bank.account(AccountOwner::User(user)).await.unwrap();
    let resume = t
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/blackjack/resume")
                .header(header::COOKIE, &cookie_value)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let resumed: serde_json::Value =
        serde_json::from_slice(&to_bytes(resume.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(resumed["id"].as_str(), Some(game_id));
    let rejected = t
        .router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/blackjack/start")
                .header(header::COOKIE, &cookie_value)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"bet":500}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(rejected.status(), StatusCode::BAD_REQUEST);
    let after = t.bank.account(AccountOwner::User(user)).await.unwrap();
    assert_eq!(before.entries, after.entries);
}
