use crate::{
    auth, bank::BankStore, blackjack::BlackjackStore, blitz::BlitzStore, driver,
    history::HistoryStore, render, routes, session::MaybeUser, stats::StatsStore,
    store::TableStore, users::UserStore,
};
use anyhow::{Context, Result};
use axum::{
    Router,
    extract::{FromRef, Request, State},
    http::{HeaderValue, header::CACHE_CONTROL},
    middleware::{Next, from_fn, from_fn_with_state},
    response::Response,
    routing::get,
};
use axum_extra::extract::cookie::Key;
use std::{env, io, sync::Arc, time::Instant};
use tokio::net::TcpListener;
use tower_http::{services::ServeDir, trace::TraceLayer};
use webauthn_rs::prelude::{Url, Webauthn, WebauthnBuilder};
const LOCAL: &str =
    "two-seven-local-development-session-secret-v1-keep-browser-sessions-across-restarts";
const DEFAULT_PORT: u16 = 8080;
const PORT_SCAN_LIMIT: u16 = 100;
#[derive(Clone)]
pub struct AppState {
    pub users: Arc<UserStore>,
    pub bank: BankStore,
    pub blackjack: BlackjackStore,
    pub blitz: BlitzStore,
    pub tables: TableStore,
    pub history: HistoryStore,
    pub stats: StatsStore,
    pub admin_password: Arc<String>,
    pub webauthn: Arc<Webauthn>,
    pub key: Key,
    pub passkey_disabled: bool,
}
impl FromRef<AppState> for Key {
    fn from_ref(s: &AppState) -> Self {
        s.key.clone()
    }
}
pub fn router(s: AppState) -> Router {
    Router::new()
        .route("/", get(routes::index))
        .route("/healthcheck", get(routes::healthcheck))
        .route("/sw.js", get(routes::service_worker))
        .route("/card-test", get(routes::card_test))
        .route("/player", get(routes::player_page))
        .route("/player/{id}", get(routes::other_player_page))
        .route(
            "/player/settings",
            axum::routing::post(routes::save_settings),
        )
        .route(
            "/player/{id}/gift",
            axum::routing::post(routes::gift_player),
        )
        .route("/leaderboard", get(routes::leaderboard))
        .route("/blackjack", get(routes::blackjack))
        .route("/admin", get(routes::admin_page).post(routes::admin_action))
        .route(
            "/blackjack/start",
            axum::routing::post(routes::blackjack_start),
        )
        .route("/blackjack/resume", get(routes::blackjack_resume))
        .route("/blackjack/hit", axum::routing::post(routes::blackjack_hit))
        .route(
            "/blackjack/stand",
            axum::routing::post(routes::blackjack_stand),
        )
        .route(
            "/blackjack/double",
            axum::routing::post(routes::blackjack_double),
        )
        .route(
            "/blackjack/split",
            axum::routing::post(routes::blackjack_split),
        )
        .route(
            "/blackjack/insurance",
            axum::routing::post(routes::blackjack_insurance),
        )
        .route("/hand-blitz", get(routes::hand_blitz))
        .route(
            "/hand-blitz/start",
            axum::routing::post(routes::hand_blitz_start),
        )
        .route("/hand-blitz/resume", get(routes::hand_blitz_resume))
        .route(
            "/hand-blitz/answer",
            axum::routing::post(routes::hand_blitz_answer),
        )
        .route("/tables/new", get(routes::new_table))
        .route("/tournaments/new", get(routes::new_tournament))
        .route(
            "/tournaments",
            axum::routing::post(routes::create_tournament),
        )
        .route(
            "/tournaments/{id}/register",
            axum::routing::post(routes::register_tournament),
        )
        .route("/tables", axum::routing::get(routes::tables))
        .route("/tables/{id}", get(routes::table_page))
        .route("/tables/{id}/state", get(routes::table_state))
        .route("/tables/{id}/history", get(routes::table_history))
        .route(
            "/tables/{id}/deal",
            axum::routing::post(routes::deal_bot_hand),
        )
        .route("/tables/{id}/events", get(routes::table_events))
        .route("/tables/{id}/join", axum::routing::post(routes::join_table))
        .route(
            "/tables/{id}/leave",
            axum::routing::post(routes::leave_table),
        )
        .route("/tables/{id}/action", axum::routing::post(routes::action))
        .route(
            "/tables/{id}/continue",
            axum::routing::post(routes::continue_table),
        )
        .route(
            "/tables/{id}/advance",
            axum::routing::post(routes::advance_runout),
        )
        .route(
            "/tables/{id}/rebuy",
            axum::routing::post(routes::rebuy_table),
        )
        .route("/tables/{id}/bot", axum::routing::post(routes::bot_table))
        .route(
            "/api/bank",
            get(routes::bank_state).post(routes::bank_re_up),
        )
        .route("/api/bank/repay", axum::routing::post(routes::bank_repay))
        .route(
            "/auth/register/begin",
            axum::routing::post(auth::register_begin),
        )
        .route(
            "/auth/register/finish",
            axum::routing::post(auth::register_finish),
        )
        .route("/auth/login/begin", axum::routing::post(auth::login_begin))
        .route(
            "/auth/login/finish",
            axum::routing::post(auth::login_finish),
        )
        .route("/auth/logout", axum::routing::post(auth::logout))
        .nest_service("/public", ServeDir::new("public"))
        .layer(from_fn(cache_control))
        .layer(from_fn_with_state(s.clone(), log_request))
        .layer(TraceLayer::new_for_http())
        .with_state(s)
}
/// Only a URL that carries the release version may be cached forever. The
/// islands import their helpers by bare path (`/public/card.js`), so those
/// requests arrive without a `?v=` and have to revalidate — otherwise an
/// installed PWA holds a year-old copy of them across releases.
fn cache_directive(path: &str, query: Option<&str>, release: bool) -> &'static str {
    let versioned = query.is_some_and(|query| {
        query
            .split('&')
            .any(|pair| pair.strip_prefix("v=").is_some_and(|v| !v.is_empty()))
    });
    if release && path.starts_with("/public/") && versioned {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    }
}

async fn cache_control(req: Request, next: Next) -> Response {
    let directive = cache_directive(req.uri().path(), req.uri().query(), !cfg!(debug_assertions));
    let mut r = next.run(req).await;
    r.headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static(directive));
    r
}
async fn log_request(State(s): State<AppState>, req: Request, next: Next) -> Response {
    let method = req.method().clone();
    let path = req.uri().path().to_string();
    let start = Instant::now();
    let r = next.run(req).await;
    let user: String = if path.starts_with("/public/") {
        "-".to_string()
    } else {
        let _ = &s;
        let _ = MaybeUser(None);
        "-".to_string()
    };
    tracing::info!(%method,path,status=r.status().as_u16(),user,elapsed_ms=start.elapsed().as_millis(),"request");
    r
}
pub async fn run() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();
    render::set_asset_version(asset_version());
    let data = env::var("DATA_PATH").unwrap_or_else(|_| "data".into());
    let users = Arc::new(UserStore::load(&data).await?);
    let (bank, house_was_reset) = BankStore::load_reporting_reset(&data).await?;
    let blackjack = BlackjackStore::load(&data).await?;
    let blitz = BlitzStore::load(&data).await?;
    let tables = TableStore::load(&data).await?;
    let history = HistoryStore::load(&data).await?;
    let stats = StatsStore::load(&data).await?;
    let admin_password = Arc::new(crate::admin::load_password(&data).await?);
    if house_was_reset {
        stats.forget_bots().await?;
    }
    let state = AppState {
        users,
        bank,
        blackjack,
        blitz,
        tables,
        history,
        stats,
        admin_password,
        webauthn: Arc::new(build_webauthn()?),
        key: load_key(),
        passkey_disabled: env_flag("PASSKEY_DISABLED"),
    };
    driver::retire_custom_cash_tables(&state).await?;
    driver::ensure_cash_ladder(&state).await?;
    driver::spawn(state.clone());
    let app = router(state);
    let listener = bind_listener().await?;
    let addr = listener
        .local_addr()
        .context("failed to read bound address")?;
    tracing::info!(
        bind_addr = %addr,
        port = addr.port(),
        "listening on http://localhost:{}",
        addr.port()
    );
    println!("listening on http://localhost:{}", addr.port());
    axum::serve(listener, app).await?;
    Ok(())
}
async fn bind_listener() -> Result<TcpListener> {
    if let Ok(port) = env::var("PORT") {
        let addr = format!("0.0.0.0:{port}");
        return TcpListener::bind(&addr)
            .await
            .with_context(|| format!("failed to bind {addr}"));
    }
    bind_open_port(DEFAULT_PORT, DEFAULT_PORT + PORT_SCAN_LIMIT).await
}
async fn bind_open_port(start: u16, end: u16) -> Result<TcpListener> {
    for port in start..=end {
        let addr = format!("0.0.0.0:{port}");
        match TcpListener::bind(&addr).await {
            Ok(listener) => return Ok(listener),
            Err(e) if e.kind() == io::ErrorKind::AddrInUse => {}
            Err(e) => return Err(e).with_context(|| format!("failed to bind {addr}")),
        }
    }
    TcpListener::bind("0.0.0.0:0")
        .await
        .context("failed to bind an ephemeral port")
}
pub fn build_webauthn() -> Result<Webauthn> {
    let id = env::var("RP_ID").unwrap_or_else(|_| "localhost".into());
    let origin =
        Url::parse(&env::var("RP_ORIGIN").unwrap_or_else(|_| "http://localhost:8080".into()))
            .context("RP_ORIGIN must be a valid URL")?;
    WebauthnBuilder::new(&id, &origin)?
        .rp_name("two-seven")
        .build()
        .context("failed to build WebAuthn")
}
fn env_flag(n: &str) -> bool {
    matches!(
        env::var(n).as_deref(),
        Ok("1") | Ok("true") | Ok("TRUE") | Ok("True")
    )
}
fn load_key() -> Key {
    match env::var("SESSION_SECRET") {
        Ok(s) if s.len() >= 64 => Key::from(s.as_bytes()),
        Ok(_) => Key::generate(),
        Err(_) => {
            if cfg!(debug_assertions) {
                Key::from(LOCAL.as_bytes())
            } else {
                Key::generate()
            }
        }
    }
}
fn asset_version() -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for f in [
        "public/css/01-tokens.css",
        "public/css/02-base.css",
        "public/css/03-controls.css",
        "public/css/04-cards.css",
        "public/css/05-table.css",
        "public/css/06-blackjack.css",
        "public/css/07-pages.css",
        "public/auth.js",
        "public/bank.js",
        "public/blackjack.js",
        "public/blitz.js",
        "public/shared.js",
        "public/card.js",
        "public/lobby.js",
        "public/player.js",
        "public/table.js",
        "public/card-settings.js",
        "public/vendor/htm-preact.js",
        "public/vendor/bitter-v42-latin.woff2",
        "public/vendor/bitter-v42-latin-ext.woff2",
        "public/manifest.webmanifest",
        "public/icon.svg",
        "public/apple-touch-icon.svg",
    ] {
        if let Ok(b) = std::fs::read(f) {
            b.hash(&mut h)
        }
    }
    format!("{:x}", h.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_versioned_assets_are_cached_forever() {
        // Entry points render with the release version and never change under it.
        assert_eq!(
            cache_directive("/public/table.js", Some("v=abc123"), true),
            "public, max-age=31536000, immutable"
        );
        // A module import arrives bare, so it must revalidate every release.
        assert_eq!(cache_directive("/public/card.js", None, true), "no-cache");
        assert_eq!(
            cache_directive("/public/card.js", Some("v="), true),
            "no-cache"
        );
        assert_eq!(
            cache_directive("/public/card.js", Some("e2e=1"), true),
            "no-cache"
        );
        // Pages and the service worker always revalidate.
        assert_eq!(
            cache_directive("/tables", Some("v=abc123"), true),
            "no-cache"
        );
        assert_eq!(cache_directive("/sw.js", None, true), "no-cache");
        // Nothing is cached in a debug build.
        assert_eq!(
            cache_directive("/public/table.js", Some("v=abc123"), false),
            "no-cache"
        );
    }

    #[tokio::test]
    async fn bind_open_port_skips_ports_in_use() {
        let occupied = std::net::TcpListener::bind("0.0.0.0:0").unwrap();
        let occupied_port = occupied.local_addr().unwrap().port();

        let listener = bind_open_port(occupied_port, occupied_port).await.unwrap();

        assert_ne!(listener.local_addr().unwrap().port(), occupied_port);
    }
}
