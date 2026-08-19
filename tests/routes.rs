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
    blackjack::max_starting_bet,
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
    tables: two_seven::store::TableStore,
    state: app::AppState,
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
    let table_store = tables.clone();
    let history = two_seven::history::HistoryStore::load(&dir).await.unwrap();
    let stats = two_seven::stats::StatsStore::load(&dir).await.unwrap();
    let key = Key::generate();
    let state = app::AppState {
        users: users.clone(),
        bank: bank.clone(),
        blackjack: two_seven::blackjack::BlackjackStore::load(&dir)
            .await
            .unwrap(),
        blitz,
        tables,
        history,
        stats,
        admin_password: Arc::new("test-admin-password".into()),
        webauthn: Arc::new(app::build_webauthn().unwrap()),
        key: key.clone(),
        passkey_disabled: true,
    };
    T {
        router: app::router(state.clone()),
        state,
        key,
        users,
        bank,
        tables: table_store,
    }
}
fn blitz_labels() -> Vec<&'static str> {
    two_seven::blitz::BlitzDifficulty::ALL
        .iter()
        .map(|difficulty| difficulty.config().label)
        .collect()
}
/// Cash tables are not created by players any more, so tests that need a
/// particular shape of table put one in the store directly.
async fn seat_table(t: &T, name: &str, seats: usize, buy_in: i64, no_debt: bool) -> Uuid {
    let table = two_seven::table::Table::new(
        name.into(),
        two_seven::table::Stakes::NoLimit {
            small_blind: 100,
            big_blind: 200,
        },
        two_seven::table::TableMode::Cash { no_debt },
        seats,
        buy_in,
    );
    t.tables.insert(table).await.unwrap()
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
async fn admin_requires_the_local_password() {
    let t = appx().await;
    let response = t
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/admin")
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from("password=wrong&action=poker"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert!(String::from_utf8_lossy(&body).contains("did not unlock"));
}

#[tokio::test]
async fn admin_money_reset_clears_accounts_and_kicks_people_from_tables() {
    let t = appx().await;
    let user = Uuid::new_v4();
    t.users
        .insert(User {
            id: user,
            username: "reset".into(),
            display_name: "Reset".into(),
            credentials: vec![],
            settings: UserSettings::default(),
            created_at: chrono::Utc::now(),
        })
        .await
        .unwrap();
    t.bank.re_up(AccountOwner::User(user)).await.unwrap();
    let table = seat_table(&t, "Reset table", 2, 20_000, false).await;
    t.tables
        .update(table, |table| {
            table.seats[0].occupant = two_seven::table::SeatOccupant::Human { user_id: user };
            table.seats[0].stack = 20_000;
            table.seats[1].occupant = two_seven::table::SeatOccupant::bot(
                two_seven::table::Bot::new(two_seven::table::BotKind::Fish, 0),
            );
            table.seats[1].stack = 20_000;
            Ok(())
        })
        .await
        .unwrap();

    let response = t
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/admin")
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from("password=test-admin-password&action=money"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert!(String::from_utf8_lossy(&body).contains("1 humans kicked out"));

    assert_eq!(
        t.bank
            .account(AccountOwner::User(user))
            .await
            .unwrap()
            .balance,
        0
    );
    let table = t.tables.get(table).await.unwrap();
    let table = table.lock().await;
    assert!(table.hand.is_none());
    assert!(table.seats.iter().all(|seat| {
        matches!(seat.occupant, two_seven::table::SeatOccupant::Empty) && seat.stack == 0
    }));
}

#[tokio::test]
async fn game_setup_walks_a_stepped_dialog() {
    let t = appx().await;
    let user = Uuid::new_v4();
    t.users
        .insert(User {
            id: user,
            username: "setup".into(),
            display_name: "Setup".into(),
            credentials: vec![],
            settings: UserSettings::default(),
            created_at: chrono::Utc::now(),
        })
        .await
        .unwrap();
    let response = t
        .router
        .oneshot(
            Request::builder()
                .uri("/tables/new")
                .header(header::COOKIE, cookie(&t.key, user))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let html = String::from_utf8_lossy(&body);
    // A stepped dialog for tournaments: players, buy-in, then confirm.
    assert_eq!(html.matches("class=\"setup-step\"").count(), 2);
    assert!(html.contains("setup-step setup-confirm"));
    assert_eq!(html.matches("data-choice=\"buyIn\"").count(), 6);
    assert!(html.contains("data-choice=\"players\""));
    assert!(
        !html.contains("data-choice=\"betting\""),
        "cash games are not created"
    );
    assert!(html.contains("quick-game-form"));
    assert!(html.contains("id=\"game-setup\""));
    assert_eq!(html.matches("setup-option-dear").count(), 0);
    assert!(html.contains(r#"<legend>Name</legend>"#));
    assert!(html.contains(r#"value="Friday night""#));
    assert!(!html.contains("Require available balance"));
}

#[tokio::test]
async fn the_lobby_is_ordered_by_buy_in_and_drops_pre_ladder_tables() {
    let t = appx().await;
    let user = Uuid::new_v4();
    t.users
        .insert(User {
            id: user,
            username: "sorter".into(),
            display_name: "Sorter".into(),
            credentials: vec![],
            settings: UserSettings::default(),
            created_at: chrono::Utc::now(),
        })
        .await
        .unwrap();
    t.bank.re_up(AccountOwner::User(user)).await.unwrap();
    let cookie_value = cookie(&t.key, user);

    // A table from before the ladder, with a player still sitting on chips.
    let legacy = seat_table(&t, "Old custom game", 6, 10_000, false).await;
    t.tables
        .update(legacy, |table| {
            table.seats[0].occupant = two_seven::table::SeatOccupant::Human { user_id: user };
            table.seats[0].stack = 10_000;
            Ok(())
        })
        .await
        .unwrap();
    let before = t
        .bank
        .account(AccountOwner::User(user))
        .await
        .unwrap()
        .balance;

    two_seven::driver::retire_custom_cash_tables(&t.state)
        .await
        .unwrap();
    two_seven::driver::ensure_cash_ladder(&t.state)
        .await
        .unwrap();
    assert!(
        t.tables.get(legacy).await.is_none(),
        "the old table is gone"
    );
    assert_eq!(
        t.bank
            .account(AccountOwner::User(user))
            .await
            .unwrap()
            .balance,
        before + 10_000,
        "chips on a retired table come back"
    );
    assert_eq!(t.tables.ids().await.len(), two_seven::cash::TIERS.len());
    t.bank
        .append(
            AccountOwner::User(user),
            LedgerKind::Adjustment,
            two_seven::cash::TIERS.last().copied().unwrap(),
            "sorting bankroll".into(),
        )
        .await
        .unwrap();

    let response = t
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/tables")
                .header(header::COOKIE, cookie_value)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let html = String::from_utf8_lossy(&body);
    assert!(!html.contains("Old custom game"));
    // Every rung appears, cheapest first.
    let mut last = 0;
    for tier in two_seven::cash::TIERS {
        let at = html
            .find(&two_seven::cash::name(tier))
            .unwrap_or_else(|| panic!("{} should be listed", two_seven::cash::name(tier)));
        assert!(at > last, "tables run cheapest first");
        last = at;
    }
}

#[tokio::test]
async fn a_table_with_nobody_at_it_waits_to_be_asked() {
    let t = appx().await;
    let user = Uuid::new_v4();
    t.users
        .insert(User {
            id: user,
            username: "spectator".into(),
            display_name: "Spectator".into(),
            credentials: vec![],
            settings: UserSettings::default(),
            created_at: chrono::Utc::now(),
        })
        .await
        .unwrap();
    let cookie_value = cookie(&t.key, user);
    two_seven::driver::ensure_cash_ladder(&t.state)
        .await
        .unwrap();
    let id = {
        let mut found = None;
        for id in t.tables.ids().await {
            let table = t.tables.get(id).await.unwrap();
            if table.lock().await.cash_tier == Some(0) {
                found = Some(id);
            }
        }
        found.expect("the cheapest table")
    };

    // Time passes, the house fills every seat, and still nothing is dealt.
    let mut now = chrono::Utc::now();
    for _ in 0..60 {
        now += chrono::Duration::seconds(1);
        two_seven::driver::tick_once_at(&t.state, now)
            .await
            .unwrap();
    }
    let seated = {
        let table = t.tables.get(id).await.unwrap();
        let table = table.lock().await;
        assert!(
            table.hand.is_none(),
            "the house does not play to an empty room"
        );
        table
            .seats
            .iter()
            .filter(|seat| seat.occupant.as_bot().is_some())
            .count()
    };
    assert_eq!(seated, two_seven::cash::SEATS, "but it does sit down");

    // A watcher asks for one hand, and gets exactly one.
    let deal = |cookie_value: String| {
        let router = t.router.clone();
        async move {
            router
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(format!("/tables/{id}/deal"))
                        .header(header::COOKIE, cookie_value)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap()
                .status()
        }
    };
    assert_eq!(deal(cookie_value.clone()).await, StatusCode::OK);
    {
        let table = t.tables.get(id).await.unwrap();
        assert!(table.lock().await.hand.is_some(), "the ask deals a hand");
    }
    // Asking again while it plays is refused rather than queued.
    assert_eq!(deal(cookie_value.clone()).await, StatusCode::BAD_REQUEST);

    // Play it out; the table then goes quiet again instead of dealing on.
    for _ in 0..400 {
        now += chrono::Duration::seconds(1);
        two_seven::driver::tick_once_at(&t.state, now)
            .await
            .unwrap();
    }
    let table = t.tables.get(id).await.unwrap();
    let table = table.lock().await;
    assert!(table.hand.is_none(), "one ask buys one hand, not a session");
    assert!(table.hand_no >= 1, "and that hand was played");
}

#[tokio::test]
async fn the_house_plays_its_way_onto_the_leaderboard() {
    let t = appx().await;
    two_seven::driver::ensure_cash_ladder(&t.state)
        .await
        .unwrap();
    // Bot actions are paced in real time, so the clock has to move with the
    // ticks for hands to actually play out.
    let mut now = chrono::Utc::now();
    for _ in 0..400 {
        now += chrono::Duration::seconds(1);
        // Nobody is seated, so each hand has to be asked for.
        for id in t.tables.ids().await {
            let _ = t
                .tables
                .update(id, |table| {
                    table.bot_hands_requested = 1;
                    Ok(())
                })
                .await;
        }
        two_seven::driver::tick_once_at(&t.state, now)
            .await
            .unwrap();
    }
    let stats = t.state.stats.all().await;
    assert!(
        stats.keys().any(|key| key.starts_with("bot:")),
        "house players accumulate a record too"
    );
    let played: u64 = stats.values().map(|player| player.hands).sum();
    assert!(played > 0, "somebody has played a hand");

    let user = Uuid::new_v4();
    t.users
        .insert(User {
            id: user,
            username: "watcher".into(),
            display_name: "Watcher".into(),
            credentials: vec![],
            settings: UserSettings::default(),
            created_at: chrono::Utc::now(),
        })
        .await
        .unwrap();
    let response = t
        .router
        .clone()
        .clone()
        .oneshot(
            Request::builder()
                .uri("/leaderboard")
                .header(header::COOKIE, cookie(&t.key, user))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let html = String::from_utf8_lossy(&body);
    // A named regular is listed, marked as the house, with poker columns.
    let roster = two_seven::table::Bot::roster();
    assert!(
        roster.iter().any(|bot| html.contains(bot.name())),
        "a house regular should be on the board"
    );
    assert!(html.contains("house-tag"));
    for header in ["Hands", "VPIP", "PFR", "Biggest pot"] {
        assert!(html.contains(header), "missing {header} column");
    }
    // Balances descend down the table.
    let balances: Vec<i64> = html
        .split("<td class=\"money\">$")
        .skip(1)
        .filter_map(|cell| {
            cell.split('<')
                .next()?
                .replace(',', "")
                .split('.')
                .next()?
                .parse::<i64>()
                .ok()
        })
        .step_by(2)
        .collect();
    assert!(
        balances.windows(2).all(|pair| pair[0] >= pair[1]),
        "the board is sorted by balance: {balances:?}"
    );
}

#[tokio::test]
async fn blackjack_caps_starting_bet_at_half_the_bankroll() {
    let t = appx().await;
    let user = Uuid::new_v4();
    t.users
        .insert(User {
            id: user,
            username: "highroller".into(),
            display_name: "High Roller".into(),
            credentials: vec![],
            settings: UserSettings::default(),
            created_at: chrono::Utc::now(),
        })
        .await
        .unwrap();
    let account = t.bank.re_up(AccountOwner::User(user)).await.unwrap();
    let cookie_value = cookie(&t.key, user);
    let deal = |bet: i64| {
        let router = t.router.clone();
        let cookie_value = cookie_value.clone();
        async move {
            router
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/blackjack/start")
                        .header(header::COOKIE, cookie_value)
                        .header(header::CONTENT_TYPE, "application/json")
                        .body(Body::from(format!(r#"{{"bet":{bet}}}"#)))
                        .unwrap(),
                )
                .await
                .unwrap()
                .status()
        }
    };
    // A dollar over the balance is refused, and the full balance is too high.
    assert_eq!(deal(account.balance + 1).await, StatusCode::BAD_REQUEST);
    assert_eq!(deal(account.balance).await, StatusCode::BAD_REQUEST);
    assert_eq!(
        deal(max_starting_bet(account.balance)).await,
        StatusCode::OK
    );
}

#[tokio::test]
async fn the_lobby_counts_humans_and_lists_cash_tables_you_can_afford() {
    let t = appx().await;
    let user = Uuid::new_v4();
    t.users
        .insert(User {
            id: user,
            username: "browser".into(),
            display_name: "Browser".into(),
            credentials: vec![],
            settings: UserSettings::default(),
            created_at: chrono::Utc::now(),
        })
        .await
        .unwrap();
    t.bank.re_up(AccountOwner::User(user)).await.unwrap();
    let cookie_value = cookie(&t.key, user);
    two_seven::driver::ensure_cash_ladder(&t.state)
        .await
        .unwrap();
    for _ in 0..(two_seven::cash::SEATS + 2) {
        two_seven::driver::tick_once(&t.state).await.unwrap();
    }

    let response = t
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/tables")
                .header(header::COOKIE, cookie_value)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let html = String::from_utf8_lossy(&body);
    // The house filled every table, so none of them holds a person yet.
    assert!(html.contains("no humans"), "the list says who is a person");
    // Cash tables and tournaments are listed apart.
    assert!(html.contains("<h2>Cash tables</h2>"));
    assert!(html.contains("<h2>Tournaments</h2>"));
    for tier in two_seven::cash::TIERS {
        let name = two_seven::cash::name(tier);
        if tier <= BankStore::RE_UP_AMOUNT {
            assert!(html.contains(&name), "{name} should be visible");
        } else {
            assert!(!html.contains(&name), "{name} should be hidden");
        }
    }
}

#[tokio::test]
async fn a_human_may_take_a_house_players_seat() {
    let t = appx().await;
    let user = Uuid::new_v4();
    t.users
        .insert(User {
            id: user,
            username: "walkin".into(),
            display_name: "Walk-in".into(),
            credentials: vec![],
            settings: UserSettings::default(),
            created_at: chrono::Utc::now(),
        })
        .await
        .unwrap();
    t.bank.re_up(AccountOwner::User(user)).await.unwrap();
    let cookie_value = cookie(&t.key, user);

    // The cheapest standing table, filled by the house.
    two_seven::driver::ensure_cash_ladder(&t.state)
        .await
        .unwrap();
    let id = {
        let mut found = None;
        for id in t.tables.ids().await {
            let table = t.tables.get(id).await.unwrap();
            if table.lock().await.cash_tier == Some(0) {
                found = Some(id);
            }
        }
        found.expect("the cheapest table")
    };
    for _ in 0..(two_seven::cash::SEATS + 2) {
        two_seven::driver::tick_once(&t.state).await.unwrap();
    }
    let seated_bots = {
        let table = t.tables.get(id).await.unwrap();
        let table = table.lock().await;
        table
            .seats
            .iter()
            .filter(|seat| seat.occupant.as_bot().is_some())
            .count()
    };
    assert_eq!(seated_bots, two_seven::cash::SEATS, "the house filled it");

    let join = t
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/tables/{id}/join"))
                .header(header::COOKIE, &cookie_value)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        join.status(),
        StatusCode::OK,
        "a full house still has room for a person"
    );
    let table = t.tables.get(id).await.unwrap();
    let table = table.lock().await;
    assert_eq!(
        table
            .seats
            .iter()
            .filter(|seat| matches!(seat.occupant, two_seven::table::SeatOccupant::Human { .. }))
            .count(),
        1
    );
    assert_eq!(
        table
            .seats
            .iter()
            .filter(|seat| seat.occupant.as_bot().is_some())
            .count(),
        two_seven::cash::SEATS - 1,
        "exactly one house player gave up their seat"
    );
}

#[tokio::test]
async fn the_leaderboard_ranks_by_balance_then_by_fewer_loans() {
    let t = appx().await;
    // Same balance, different borrowing: the one who took fewer loans is above.
    let thrifty = Uuid::new_v4();
    let borrower = Uuid::new_v4();
    let poorest = Uuid::new_v4();
    for (id, name) in [
        (thrifty, "Thrifty"),
        (borrower, "Borrower"),
        (poorest, "Poorest"),
    ] {
        t.users
            .insert(User {
                id,
                username: name.to_lowercase(),
                display_name: name.into(),
                credentials: vec![],
                settings: UserSettings::default(),
                created_at: chrono::Utc::now(),
            })
            .await
            .unwrap();
    }
    let spend = async |user: Uuid, amount: i64| {
        t.bank
            .append(
                AccountOwner::User(user),
                LedgerKind::Adjustment,
                -amount,
                "spent".into(),
            )
            .await
            .unwrap();
    };
    // A re-up is the loan, and it is only available once nearly broke, so the
    // borrower has to go bust before taking a second one.
    t.bank.re_up(AccountOwner::User(thrifty)).await.unwrap();
    t.bank.re_up(AccountOwner::User(borrower)).await.unwrap();
    spend(borrower, 95_000).await;
    t.bank.re_up(AccountOwner::User(borrower)).await.unwrap();
    spend(borrower, 5_000).await;
    t.bank.re_up(AccountOwner::User(poorest)).await.unwrap();
    spend(poorest, 50_000).await;

    let cookie_value = cookie(&t.key, thrifty);
    let response = t
        .router
        .oneshot(
            Request::builder()
                .uri("/leaderboard")
                .header(header::COOKIE, &cookie_value)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let html = String::from_utf8_lossy(&body);
    let place = |name: &str| html.find(name).unwrap_or(usize::MAX);
    assert!(
        place("Thrifty") < place("Borrower"),
        "equal balances break toward fewer loans"
    );
    assert!(
        place("Borrower") < place("Poorest"),
        "a bigger balance still outranks"
    );
    // Every difficulty gets its own accuracy and streak columns.
    for difficulty in blitz_labels() {
        assert!(html.contains(difficulty), "missing {difficulty} columns");
    }
    assert_eq!(html.matches("<th>Accuracy</th><th>Streak</th>").count(), 3);
    // Bots bankroll themselves and are not in the running.
    assert!(!html.contains("fish"));
}

#[tokio::test]
async fn signing_out_is_quiet_and_asks_first() {
    let t = appx().await;
    let user = Uuid::new_v4();
    t.users
        .insert(User {
            id: user,
            username: "leaver".into(),
            display_name: "Leaver".into(),
            credentials: vec![],
            settings: UserSettings::default(),
            created_at: chrono::Utc::now(),
        })
        .await
        .unwrap();
    let cookie_value = cookie(&t.key, user);
    // The signed-in home is the only page that offers a way out.
    for path in ["/"] {
        let response = t
            .router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(path)
                    .header(header::COOKIE, &cookie_value)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let html = String::from_utf8_lossy(&body);
        // The trigger only opens a dialog; the form is submitted from inside it.
        assert!(html.contains("sign-out-trigger"), "{path} needs a trigger");
        assert!(
            html.contains(r#"<button class="sign-out-trigger" type="button">"#),
            "{path}: the trigger must not submit the form"
        );
        assert!(
            html.contains(r#"<dialog id="sign-out" class="confirm-dialog">"#),
            "{path} needs a confirmation"
        );
        assert!(
            html.contains(r#"<button class="danger" type="submit">Sign out</button>"#),
            "{path}: only the confirming button submits"
        );
        assert_eq!(
            html.matches(r#"action="/auth/logout""#).count(),
            1,
            "{path}: one way out is enough"
        );
    }
}

#[tokio::test]
async fn every_finished_hand_lands_in_the_table_history() {
    let t = appx().await;
    let user = Uuid::new_v4();
    t.users
        .insert(User {
            id: user,
            username: "historian".into(),
            display_name: "Historian".into(),
            credentials: vec![],
            settings: UserSettings::default(),
            created_at: chrono::Utc::now(),
        })
        .await
        .unwrap();
    let cookie_value = cookie(&t.key, user);
    t.bank.re_up(AccountOwner::User(user)).await.unwrap();
    let id = seat_table(&t, "Logged", 2, 10_000, false).await;

    // Nothing has been played, so the log is empty but the page still renders.
    let empty = history_json(&t, id, &cookie_value).await;
    assert_eq!(empty["total"].as_u64(), Some(0));
    assert_eq!(empty["hands"].as_array().unwrap().len(), 0);

    for path in ["join", "bot"] {
        let payload = if path == "bot" {
            r#"{"seat":1,"kind":"rock"}"#
        } else {
            "{}"
        };
        let response = t
            .router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/tables/{id}/{path}"))
                    .header(header::COOKIE, &cookie_value)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(payload))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{path} should succeed");
    }

    // Folding ends the hand, which is the moment history is written.
    let fold = t
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/tables/{id}/action"))
                .header(header::COOKIE, &cookie_value)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"kind":"fold"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(fold.status(), StatusCode::OK);

    let logged = history_json(&t, id, &cookie_value).await;
    assert_eq!(logged["total"].as_u64(), Some(1));
    let hand = &logged["hands"][0];
    assert_eq!(hand["hand_no"].as_u64(), Some(1));
    // The record keeps every hole card, including the hand that folded, which
    // the live table view redacts.
    let seats = hand["seats"].as_array().unwrap();
    assert_eq!(seats.len(), 2);
    for seat in seats {
        assert_eq!(seat["hole_cards"].as_array().unwrap().len(), 2);
        // The occupant is structured now, so history says who by identity.
        assert!(
            seat["occupant"].get("Human").is_some() || seat["occupant"].get("Bot").is_some(),
            "occupant should name a person or a house player: {}",
            seat["occupant"]
        );
    }
    assert!(!hand["summary"]["events"].as_array().unwrap().is_empty());

    // The page names the table and lists the hand.
    let page = t
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/tables/{id}/history"))
                .header(header::COOKIE, &cookie_value)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(page.status(), StatusCode::OK);
    let page = to_bytes(page.into_body(), usize::MAX).await.unwrap();
    let page = String::from_utf8_lossy(&page);
    assert!(page.contains("Logged"));
    assert!(page.contains("Hand 1"));
    assert!(page.contains("hand-record"));
}

#[tokio::test]
async fn table_history_downloads_explicit_json_but_keeps_accept_json_inline() {
    let t = appx().await;
    let user = Uuid::new_v4();
    t.users
        .insert(User {
            id: user,
            username: "history-download".into(),
            display_name: "History Download".into(),
            credentials: vec![],
            settings: UserSettings::default(),
            created_at: chrono::Utc::now(),
        })
        .await
        .unwrap();
    let cookie_value = cookie(&t.key, user);
    let id = seat_table(&t, "History\r\nDownload", 2, 10_000, false).await;

    let download = t
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/tables/{id}/history?format=json"))
                .header(header::COOKIE, &cookie_value)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(download.status(), StatusCode::OK);
    let disposition = download
        .headers()
        .get(header::CONTENT_DISPOSITION)
        .unwrap()
        .to_str()
        .unwrap();
    assert_eq!(
        disposition,
        format!("attachment; filename=\"History__Download-{id}.json\"")
    );
    assert_eq!(
        download.headers().get(header::CONTENT_TYPE).unwrap(),
        "application/json"
    );
    let body = to_bytes(download.into_body(), usize::MAX).await.unwrap();
    let _: serde_json::Value = serde_json::from_slice(&body).unwrap();

    let inline = t
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/tables/{id}/history"))
                .header(header::COOKIE, &cookie_value)
                .header(header::ACCEPT, "application/json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(inline.status(), StatusCode::OK);
    assert!(inline.headers().get(header::CONTENT_DISPOSITION).is_none());
    let body = to_bytes(inline.into_body(), usize::MAX).await.unwrap();
    let _: serde_json::Value = serde_json::from_slice(&body).unwrap();
}

async fn history_json(t: &T, id: Uuid, cookie_value: &str) -> serde_json::Value {
    let response = t
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/tables/{id}/history"))
                .header(header::COOKIE, cookie_value)
                .header(header::ACCEPT, "application/json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap()
}

#[tokio::test]
async fn the_lobby_drops_a_finished_tournament() {
    let t = appx().await;
    let user = Uuid::new_v4();
    t.users
        .insert(User {
            id: user,
            username: "lobby".into(),
            display_name: "Lobby".into(),
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
                .uri("/tournaments")
                .header(header::COOKIE, &cookie_value)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"name":"Last night","buy_in":50000,"seat_count":2,"starting_chips":1000000,"levels":[{"small_blind":10000,"big_blind":20000,"ante":0,"hands":4}],"payout_percentages":[100]}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(create.status(), StatusCode::OK);
    let body: serde_json::Value =
        serde_json::from_slice(&to_bytes(create.into_body(), usize::MAX).await.unwrap()).unwrap();
    let id: Uuid = body["id"].as_str().unwrap().parse().unwrap();

    let lobby = |router: Router, cookie_value: String| async move {
        let response = router
            .oneshot(
                Request::builder()
                    .uri("/tables")
                    .header(header::COOKIE, cookie_value)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        String::from_utf8_lossy(&to_bytes(response.into_body(), usize::MAX).await.unwrap())
            .into_owned()
    };
    assert!(
        lobby(t.router.clone(), cookie_value.clone())
            .await
            .contains("Last night")
    );

    t.tables
        .update(id, |table| {
            if let two_seven::table::TableMode::Tournament(state) = &mut table.mode {
                state.finished = true;
            }
            Ok(())
        })
        .await
        .unwrap();
    assert!(
        !lobby(t.router.clone(), cookie_value)
            .await
            .contains("Last night"),
        "a finished tournament must drop out of the lobby"
    );
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
async fn game_entries_enforce_a_one_dollar_floor_and_a_million_dollar_ceiling() {
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
                    r#"{"name":"Huge","buy_in":100000001,"seat_count":4,"starting_chips":10000,"levels":[{"small_blind":100,"big_blind":200,"ante":0,"hands":10}],"payout_percentages":[100]}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(huge_tournament.status(), StatusCode::BAD_REQUEST);

    // Under a dollar, and more than the whole bankroll.
    for bet in [99, 100_000_001] {
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
    t.bank.re_up(AccountOwner::User(id)).await.unwrap();
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
    t.bank.re_up(AccountOwner::User(id)).await.unwrap();
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
    for user in [alice, bob] {
        t.bank.re_up(AccountOwner::User(user)).await.unwrap();
    }
    let id = seat_table(&t, "Test", 2, 10_000, false).await;
    let table_page = t
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/tables/{id}"))
                .header(header::COOKIE, &cookie_a)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(table_page.status(), StatusCode::OK);
    let table_html = String::from_utf8(
        to_bytes(table_page.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(table_html.contains(r#"<header class="site-header">"#));
    assert!(table_html.contains(r#"class="table-config-button""#));
    assert!(table_html.contains(r#"aria-label="Card display settings""#));
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
                .body(Body::from(r#"{"kind":"fish"}"#))
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
    assert_eq!(account["balance"], 99_900);
    assert_eq!(account["entries"].as_array().unwrap().len(), 3);
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
    t.bank.re_up(AccountOwner::User(user)).await.unwrap();
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
        95_000
    );
}

#[tokio::test]
async fn cash_bot_buy_in_auto_re_ups_without_debt() {
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
    let id = seat_table(&t, "No debt bots", 9, 10_000, true).await;
    let response = t
        .router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/tables/{id}/bot"))
                .header(header::COOKIE, &cookie_value)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"seat":0,"kind":"fish"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let account = t
        .bank
        .account(AccountOwner::Bot(two_seven::table::Bot::new(
            two_seven::table::BotKind::Fish,
            0,
        )))
        .await
        .unwrap();
    assert_eq!(account.balance, 90_000);
    assert_eq!(account.loan_count, 1);
    let table = t.tables.get(id).await.unwrap();
    let table = table.lock().await;
    assert_eq!(
        table.seats[0].occupant.as_bot(),
        Some(two_seven::table::Bot::new(
            two_seven::table::BotKind::Fish,
            0
        ))
    );
}

#[tokio::test]
async fn cash_join_requires_available_balance() {
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
    let id = seat_table(&t, "No debt", 9, 10_000, true).await;
    let response = t
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/tables/{id}/join"))
                .header(header::COOKIE, &cookie_value)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let open_id = seat_table(&t, "Open debt", 9, 10_000, false).await;
    let response = t
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/tables/{open_id}/join"))
                .header(header::COOKIE, &cookie_value)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    t.bank.re_up(AccountOwner::User(user)).await.unwrap();
    let response = t
        .router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/tables/{open_id}/join"))
                .header(header::COOKIE, cookie_value)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn cash_rebuy_requires_available_balance() {
    let t = appx().await;
    let user = Uuid::new_v4();
    t.users
        .insert(User {
            id: user,
            username: "Rebuyer".into(),
            display_name: "Rebuyer".into(),
            credentials: vec![],
            settings: UserSettings::default(),
            created_at: chrono::Utc::now(),
        })
        .await
        .unwrap();
    let cookie_value = cookie(&t.key, user);
    t.bank.re_up(AccountOwner::User(user)).await.unwrap();
    let id = seat_table(&t, "Rebuy", 9, BankStore::RE_UP_AMOUNT, false).await;

    let join = t
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/tables/{id}/join"))
                .header(header::COOKIE, &cookie_value)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(join.status(), StatusCode::OK);

    t.tables
        .update(id, |table| {
            let seat = table
                .seats
                .iter_mut()
                .find(|seat| {
                    matches!(seat.occupant, two_seven::table::SeatOccupant::Human { user_id } if user_id == user)
                })
                .expect("user seat");
            seat.stack = 0;
            Ok(())
        })
        .await
        .unwrap();

    let rebuy = t
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/tables/{id}/rebuy"))
                .header(header::COOKIE, &cookie_value)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(rebuy.status(), StatusCode::BAD_REQUEST);

    t.bank.re_up(AccountOwner::User(user)).await.unwrap();
    let rebuy = t
        .router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/tables/{id}/rebuy"))
                .header(header::COOKIE, cookie_value)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(rebuy.status(), StatusCode::OK);
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
    t.bank.re_up(AccountOwner::User(user)).await.unwrap();
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
    t.bank.re_up(AccountOwner::User(user)).await.unwrap();
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
