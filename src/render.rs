use crate::money::{Cents, format_cents};
use std::sync::OnceLock;
use uuid::Uuid;
static VERSION: OnceLock<String> = OnceLock::new();
pub fn set_asset_version(v: String) {
    let _ = VERSION.set(v);
}
fn asset(p: &str) -> String {
    format!(
        "{p}?v={}",
        VERSION.get().map(String::as_str).unwrap_or("dev")
    )
}
/// Islands import their helpers by bare path, which would otherwise be fetched
/// without the release version and cached across deploys. An import map rewrites
/// those specifiers to versioned URLs, so a release reaches every module.
fn import_map() -> String {
    let entries = [
        "/public/card.js",
        "/public/card-settings.js",
        "/public/shared.js",
        "/public/vendor/htm-preact.js",
    ]
    .iter()
    .map(|module| format!(r#""{module}":"{}""#, asset(module)))
    .collect::<Vec<_>>()
    .join(",");
    format!(r#"<script type="importmap">{{"imports":{{{entries}}}}}</script>"#)
}

/// Signing out is a quiet, out-of-the-way control with a confirmation behind
/// it: it is easy to hit by accident and there is no undo.
fn sign_out() -> &'static str {
    concat!(
        r#"<form class="sign-out" method="post" action="/auth/logout">"#,
        r#"<button class="sign-out-trigger" type="button" commandfor="sign-out" command="show-modal">Sign out</button>"#,
        r#"<dialog id="sign-out" class="confirm-dialog"><div><header><h2>Sign out?</h2></header>"#,
        r#"<p>You will have to sign in again. Any table you are sitting at keeps your seat.</p>"#,
        r#"<footer><button class="sign-out-cancel" type="button" commandfor="sign-out" command="close">Stay signed in</button>"#,
        r#"<button class="danger" type="submit">Sign out</button></footer></div></dialog></form>"#
    )
}

pub fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
pub fn layout(title: &str, body: &str, head: &str) -> String {
    layout_with_header(title, body, head, None, "")
}

fn layout_with_header(
    title: &str,
    body: &str,
    head: &str,
    context: Option<&str>,
    header_extra: &str,
) -> String {
    let context = context.map_or_else(String::new, |value| {
        format!(r#"<span class="header-context">{}</span>"#, escape(value))
    });
    format!(
        r##"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1,maximum-scale=1,user-scalable=no,viewport-fit=cover"><meta name="theme-color" content="#123d34"><meta name="apple-mobile-web-app-capable" content="yes"><meta name="apple-mobile-web-app-status-bar-style" content="black-translucent"><title>{}</title><link rel="manifest" href="{}"><link rel="icon" href="{}"><link rel="apple-touch-icon" href="{}"><link rel="preload" href="/public/vendor/bitter-v42-latin.woff2" as="font" type="font/woff2" crossorigin><link rel="stylesheet" href="{}"><link rel="stylesheet" href="{}"><link rel="stylesheet" href="{}"><link rel="stylesheet" href="{}"><link rel="stylesheet" href="{}"><link rel="stylesheet" href="{}"><link rel="stylesheet" href="{}">{}{}</head><body><main class="page"><header class="site-header"><a class="brand" href="/">♠ two-seven</a>{}<details class="bank-widget" title="Account balance"><summary>🪙 <span id="bank-balance">—</span><span id="bank-delta"></span></summary><div id="bank-panel" class="bank-panel" role="status"></div></details>{}</header>{}</main><script type="module" src="{}" defer></script></body></html>"##,
        escape(title),
        asset("/public/manifest.webmanifest"),
        asset("/public/icon.svg"),
        asset("/public/apple-touch-icon.svg"),
        asset("/public/css/01-tokens.css"),
        asset("/public/css/02-base.css"),
        asset("/public/css/03-controls.css"),
        asset("/public/css/04-cards.css"),
        asset("/public/css/05-table.css"),
        asset("/public/css/06-blackjack.css"),
        asset("/public/css/07-pages.css"),
        import_map(),
        head,
        context,
        header_extra,
        body,
        asset("/public/bank.js")
    )
}
pub fn error_page(t: &str, m: &str) -> String {
    layout(
        t,
        &format!(
            "<section class=card><h1>{}</h1><p>{}</p><a href=\"/\">Back home</a></section>",
            escape(t),
            escape(m)
        ),
        "",
    )
}
pub fn home(signed: Option<(Uuid, String)>) -> String {
    match signed {
        None => layout(
            "two-seven",
            r#"<section class="card"><h1>two-seven</h1><p>Texas Hold'em, for play money.</p><p id="auth-error" class="error" hidden></p><form id="register-form"><h2>Register</h2><input name="username" required maxlength="32" placeholder="Username"><input name="display_name" maxlength="48" placeholder="Display name"><button>Register</button></form><form id="login-form"><h2>Sign in</h2><input name="username" required placeholder="Username"><button>Sign in</button></form></section>"#,
            &format!(
                r#"<script type="module" src="{}" defer></script>"#,
                asset("/public/auth.js")
            ),
        ),
        Some((_, name)) => layout(
            "two-seven",
            &format!(
                r#"<section class="card"><h1>Welcome, {}</h1><p>Play Texas Hold'em at a cash table.</p><p><a href="/tables">Open lobby</a> · <a href="/player">Player</a> · <a href="/hand-blitz">Hand Blitz</a> · <a href="/blackjack">Blackjack</a> · <a href="/leaderboard">Leaderboard</a> · <a href="/tables/new">Start a game</a></p><form class="re-up-form"><button type="submit">Re-up $1,000</button></form>{}</section>"#,
                escape(&name),
                sign_out()
            ),
            "",
        ),
    }
}

pub fn home_lobby(name: &str, tables: &[crate::view::LobbyTableView], _balance: Cents) -> String {
    layout(
        "Lobby",
        &format!(
            "<section class=\"card lobby\"><h1>Welcome, {}</h1>{}<p><a href=\"/player\">Player</a> · <a href=\"/hand-blitz\">Hand Blitz</a> · <a href=\"/blackjack\">Blackjack</a> · <a href=\"/leaderboard\">Leaderboard</a> · <a href=\"/tables/new\">Start a game</a></p>{}</section>",
            escape(name),
            lobby_table_list(tables, true),
            sign_out()
        ),
        "",
    )
}

fn classed_option(extra: &str, name: &str, value: &str, title: &str, detail: &str) -> String {
    format!(
        r#"<button class="setup-option{extra}" type="button" data-choice="{name}" value="{value}"><b>{title}</b><small>{detail}</small></button>"#
    )
}

pub fn table_create(balance: Cents) -> String {
    game_create(balance, false)
}

pub fn tournament_create(balance: Cents, unfunded: bool) -> String {
    game_create(balance, unfunded)
}

/// `unfunded` is the account option that lets you set up a tournament richer
/// than you are. The rungs above your balance are offered rather than hidden,
/// and say what they are, so the step stays honest about what you can afford.
fn game_create(balance: Cents, unfunded: bool) -> String {
    // One question per step; lobby.js walks the steps and assembles the config.
    let step = |name: &str, legend: &str, options: &str| {
        format!(
            r#"<fieldset class="setup-step" data-step="{name}" hidden><legend>{legend}</legend><div class="setup-options">{options}</div></fieldset>"#
        )
    };
    let option = |name: &str, value: &str, title: &str, detail: &str| {
        classed_option("", name, value, title, detail)
    };
    let players_step = step(
        "players",
        "How many players?",
        &format!(
            "{}{}{}",
            option(
                "players",
                "4",
                "4 players",
                "Winner takes the whole prize pool",
            ),
            option("players", "6", "6 players", "Top 2 paid"),
            option("players", "9", "9 players", "Top 3 paid")
        ),
    );
    let buy_ins = crate::cash::TIERS
        .into_iter()
        .filter(|amount| unfunded || *amount <= balance)
        .collect::<Vec<_>>();
    let buy_in_step = step(
        "buyIn",
        "How much to buy in?",
        &if buy_ins.is_empty() {
            r#"<p class="setup-note">You need at least $200 to start a tournament.</p>"#.to_string()
        } else {
            buy_ins
                .iter()
                .map(|amount| {
                    if *amount > balance {
                        classed_option(
                            " setup-option-dear",
                            "buyIn",
                            &amount.to_string(),
                            &format_cents(*amount),
                            "10,000 chips · more than your balance",
                        )
                    } else {
                        option(
                            "buyIn",
                            &amount.to_string(),
                            &format_cents(*amount),
                            "10,000 chips · blinds climb every few hands",
                        )
                    }
                })
                .collect::<String>()
        },
    );
    let confirm_step = r#"<fieldset class="setup-step setup-confirm" data-step="confirm" hidden><legend>Name</legend><p class="setup-summary" id="setup-summary"></p><label>Name<input name="name" required maxlength="48" value="Friday night"></label><button class="setup-create" type="submit">Create tournament</button></fieldset>"#;
    let body = format!(
        r#"<section class="setup-shell"><dialog id="game-setup" class="setup-dialog"><form id="quick-game-form"><header><h2 id="setup-title">Start a tournament</h2><a class="setup-close" href="/tables" aria-label="Cancel">×</a></header><p class="setup-note">Cash games run around the clock in the lobby. A tournament is the one you start yourself.</p>{players_step}{buy_in_step}{confirm_step}<footer><button class="setup-back" type="button" hidden>Back</button><p id="create-error" class="error" role="alert"></p></footer></form></dialog><script type="module" src="{lobby}" defer></script></section>"#,
        lobby = asset("/public/lobby.js")
    );
    layout("Start a tournament", &body, "")
}

pub fn hand_blitz(stats: &crate::blitz::BlitzStats) -> String {
    let difficulties = crate::blitz::BlitzDifficulty::ALL
        .iter()
        .map(|difficulty| {
            let config = difficulty.config();
            format!(
                r#"<button type="button" data-difficulty="{}"><b>{}</b><span>{} buy-in · {}s</span></button>"#,
                config.id,
                config.label,
                format_cents(config.buy_in),
                config.time_limit_ms / 1_000
            )
        })
        .collect::<String>();
    layout(
        "Hand Blitz",
        &format!(
            r#"<section class="blitz-shell"><div class="blitz-top"><div><h1>Hand Blitz</h1><p>Pick the winning Hold'em hand before the clock runs out.</p></div><a href="/tables">Lobby</a></div><div id="blitz-app" data-stats-runs="{}" data-stats-attempts="{}" data-stats-correct="{}" data-stats-avg-ms="{}" data-stats-best="{}"><section class="blitz-menu"><div class="blitz-stat-grid"><span><b>{}</b> avg</span><span><b>{}%</b> accuracy</span><span><b>{}</b> best</span></div><div class="difficulty-grid">{}</div></section></div></section>"#,
            stats.runs,
            stats.attempts,
            stats.correct,
            stats.avg_answer_ms(),
            stats.best_streak,
            format_duration_ms(stats.avg_answer_ms()),
            stats.accuracy_percent(),
            stats.best_streak,
            difficulties
        ),
        &format!(
            r#"<script type="module" src="{}" defer></script>"#,
            asset("/public/blitz.js")
        ),
    )
}

pub fn blackjack() -> String {
    layout_with_header(
        "Blackjack",
        r#"<section class="blackjack-shell"><div id="blackjack-app"><section class="blackjack-table"><div class="actions blackjack-actions"><span class="deal-broke">Loading your stakes…</span></div></section></div></section>"#,
        &format!(
            r#"<script type="module" src="{}" defer></script>"#,
            asset("/public/blackjack.js")
        ),
        Some("Blackjack"),
        r#"<button class="table-config-button" type="button" title="Card display settings" aria-label="Card display settings" commandfor="card-config" command="show-modal">⚙</button>"#,
    )
}

pub fn admin(error: Option<&str>, message: Option<&str>) -> String {
    let notice = message.map_or_else(String::new, |value| {
        format!(
            r#"<p class="admin-message" role="status">{}</p>"#,
            escape(value)
        )
    });
    let error = error.map_or_else(String::new, |value| {
        format!(r#"<p class="error" role="alert">{}</p>"#, escape(value))
    });
    layout(
        "Admin",
        &format!(
            r#"<section class="card admin-panel"><h1>Admin</h1>{notice}{error}<form method="post" action="/admin"><label>Secret password<input type="password" name="password" autocomplete="current-password" required autofocus></label><div class="admin-actions"><button class="danger" type="submit" name="action" value="money">Reset all money and loans</button><button type="submit" name="action" value="forgive-bot-loans">Forgive all bot loans</button><button class="danger" type="submit" name="action" value="poker">Reset all poker stats</button><button class="danger" type="submit" name="action" value="blitz">Reset all blitz stats</button><button class="danger" type="submit" name="action" value="blackjack">Reset all blackjack stats</button></div></form><p><a href="/tables">Lobby</a></p></section>"#
        ),
        "",
    )
}

pub fn card_test() -> String {
    let cards = ["s", "h", "c", "d"]
        .into_iter()
        .map(|suit| {
            let cards = ["2", "3", "4", "5", "6", "7", "8", "9", "T", "J", "Q", "K", "A"]
                .into_iter()
                .map(|rank| card_face(rank, suit))
                .collect::<String>();
            format!(
                r#"<section class="card-test-suit"><h2>{}</h2><div class="card-test-grid">{}</div></section>"#,
                suit_name(suit),
                cards
            )
        })
        .collect::<String>();
    layout(
        "Card Test",
        &format!(
            r#"<section class="blitz-shell card-test"><div class="blitz-top"><div><h1>Card Test</h1><p>Every card rendered with the in-game card face styles.</p></div><a href="/hand-blitz">Hand Blitz</a></div>{}</section>"#,
            cards
        ),
        "",
    )
}

/// What the page needs to offer somebody money: who they are, and what the
/// viewer has to give.
pub struct GiftPanel {
    pub player_id: Uuid,
    pub player_name: String,
    pub your_balance: Cents,
}

/// One counterparty in the gift ledger, named and totalled, ready to draw.
pub struct GiftPeer {
    /// Somebody's page to link to; a bot has none.
    pub id: Option<Uuid>,
    pub name: String,
    pub received: Cents,
    pub sent: Cents,
}
impl GiftPeer {
    fn net(&self) -> Cents {
        self.sent - self.received
    }
}

/// The parts of a player page that depend on who is looking: the way to pay
/// somebody else, the options only you may change, and who has traded gifts
/// with the account on show.
pub struct PlayerPanels<'a> {
    pub gift: Option<&'a GiftPanel>,
    pub settings: Option<&'a crate::users::UserSettings>,
    pub gift_peers: &'a [GiftPeer],
}

pub fn player_page(
    id: Uuid,
    name: &str,
    account: &crate::bank::Account,
    poker: crate::stats::PlayerStats,
    blitz: &crate::blitz::BlitzStats,
    panels: &PlayerPanels<'_>,
) -> String {
    let entries = account
        .entries
        .iter()
        .rev()
        .take(20)
        .map(|entry| {
            format!(
                r#"<tr><td>{}</td><td>{}</td><td class="money {}">{}</td><td class="money">{}</td></tr>"#,
                entry.at.format("%Y-%m-%d %H:%M"),
                escape(&entry.memo),
                if entry.delta >= 0 { "positive" } else { "negative" },
                signed_cents(entry.delta),
                format_cents(entry.balance_after),
            )
        })
        .collect::<String>();
    let ledger = if entries.is_empty() {
        r#"<p class="loading">No finance history yet.</p>"#.to_string()
    } else {
        format!(
            r#"<table class="finance-ledger"><thead><tr><th>When</th><th>Event</th><th>Change</th><th>Balance</th></tr></thead><tbody>{entries}</tbody></table>"#
        )
    };
    // Your own page reads as yours; somebody else's says whose it is and
    // offers a way back to your own.
    let PlayerPanels {
        gift,
        settings,
        gift_peers,
    } = *panels;
    let (blurb, nav) = match gift {
        None => (
            "Your bankroll over time.".to_string(),
            r#"<a href="/tables">Lobby</a> · <a href="/leaderboard">Leaderboard</a>"#,
        ),
        Some(_) => (
            format!("{}&#39;s bankroll over time.", escape(name)),
            r#"<a href="/player">Your page</a> · <a href="/tables">Lobby</a> · <a href="/leaderboard">Leaderboard</a>"#,
        ),
    };
    let body = format!(
        r#"<section class="player-page" data-player-id="{}"><header class="history-top"><div><h1>{}</h1><p>{}</p></div><nav>{}</nav></header><section class="player-summary"><span><small>Balance</small><b>{}</b></span><span><small>Debt</small><b>{}</b></span><span><small>Net</small><b>{}</b></span><span><small>Loans</small><b>{}</b></span><span><small>Poker net</small><b>{}</b></span><span><small>Blitz accuracy</small><b>{}%</b></span></section>{}{}{}{}<section class="finance-panel chart-panel"><h2>Finances</h2>{}</section><section class="finance-panel ledger-panel"><h2>Recent ledger</h2>{}</section></section>"#,
        id,
        escape(name),
        blurb,
        nav,
        format_cents(account.balance),
        format_cents(account.loan_debt()),
        format_cents(account.net_balance()),
        account.loan_count,
        format_cents(poker.net),
        blitz.accuracy_percent(),
        if settings.is_some() {
            loans_panel(account)
        } else {
            String::new()
        },
        gift.map_or_else(String::new, gift_panel),
        settings.map_or_else(String::new, options_panel),
        gifts_panel(gift.is_none(), name, gift_peers),
        finance_chart(account),
        ledger
    );
    layout(
        &format!("{name} player"),
        &body,
        &format!(
            r#"<script type="module" src="{}" defer></script>"#,
            asset("/public/player.js")
        ),
    )
}

/// Your own debt, and the one button that clears as much of it as the balance
/// covers. Paying loans off one at a time is the same money; nobody wants to
/// press it eleven times (§V16).
fn loans_panel(account: &crate::bank::Account) -> String {
    let unit = crate::bank::BankStore::RE_UP_AMOUNT;
    let loans = account.loan_count;
    let repayable = account.repayable_loans();
    let action = if loans == 0 {
        r#"<p class="loans-status" role="status">Nothing outstanding.</p>"#.to_string()
    } else {
        format!(
            r#"<button class="bank-action loans-repay-all" type="button"{}>{}</button><p class="loans-status" role="status">{}</p>"#,
            if repayable == 0 { " disabled" } else { "" },
            if repayable <= 1 {
                format!("Pay off one loan ({})", format_cents(unit))
            } else {
                format!(
                    "Pay off {repayable} loans ({})",
                    format_cents(repayable as Cents * unit)
                )
            },
            if repayable == 0 {
                format!(
                    "You need {} in the bank to clear a loan.",
                    format_cents(unit)
                )
            } else if repayable < loans {
                format!(
                    "That leaves {} still owed.",
                    format_cents((loans - repayable) as Cents * unit)
                )
            } else {
                "That clears the lot.".to_string()
            },
        )
    };
    format!(
        r#"<section class="finance-panel loans-panel" data-loans="{loans}" data-repayable="{repayable}"><h2>Loans</h2><p>You owe <b class="loans-debt">{}</b>{}. Every loan is {} to pay back.</p>{action}</section>"#,
        format_cents(account.loan_debt()),
        match loans {
            0 => String::new(),
            1 => " on one loan".to_string(),
            _ => format!(" across {loans} loans"),
        },
        format_cents(unit),
    )
}

/// The options that only the server can honour: one lifts a check on what you
/// may create, the other decides what the table is allowed to show you.
fn options_panel(settings: &crate::users::UserSettings) -> String {
    let checked = |on: bool| if on { " checked" } else { "" };
    format!(
        r#"<section class="finance-panel options-panel"><h2>Options</h2><label class="option-toggle"><input type="checkbox" name="unfunded-tournaments"{}><span><b>Create tournaments you cannot buy into</b><small>Set up a tournament above your balance. Registering for one still costs the buy-in.</small></span></label><label class="option-toggle"><input type="checkbox" name="see-bot-cards"{}><span><b>See the bots&#39; cards</b><small>Only while watching a table with nobody seated, you included. Anyone sitting down, even you, turns it back off.</small></span></label><p class="option-status" role="status"></p></section>"#,
        checked(settings.unfunded_tournaments),
        checked(settings.see_bot_cards),
    )
}

/// Money changes hands in whole $1,000 chips, so the control is a stepper
/// rather than a free-text amount: there is no wrong number to type.
fn gift_panel(gift: &GiftPanel) -> String {
    let increment = crate::bank::BankStore::GIFT_INCREMENT;
    let controls = if gift.your_balance >= increment {
        format!(
            r#"<div class="gift-controls"><button class="gift-step" type="button" data-step="-1" aria-label="Send $1,000 less">−</button><output class="gift-amount">{}</output><button class="gift-step" type="button" data-step="1" aria-label="Send $1,000 more">+</button><button class="gift-send" type="button">Send</button></div><p class="gift-status" role="status"></p>"#,
            format_cents(increment)
        )
    } else {
        r#"<p class="gift-status" role="status">You need at least $1,000 in the bank to send anything.</p>"#.to_string()
    };
    format!(
        r#"<section class="finance-panel gift-panel" data-player-id="{}" data-increment="{increment}" data-balance="{}"><h2>Send money</h2><p>Hand {} chips out of your own account, in $1,000 units. You have <b class="gift-balance">{}</b>.</p>{controls}</section>"#,
        gift.player_id,
        gift.your_balance,
        escape(&gift.player_name),
        format_cents(gift.your_balance),
    )
}

/// Who money has passed between, netted per person. Giving reads as the good
/// direction, so the net is what you are down to them. Sent and received both
/// show, because the net alone hides a pair that keeps handing the same chips
/// back and forth.
fn gifts_panel(is_own_page: bool, name: &str, peers: &[GiftPeer]) -> String {
    if peers.is_empty() {
        return String::new();
    }
    let rows = peers
        .iter()
        .map(|peer| {
            let who = match peer.id {
                Some(id) => format!(r#"<a href="/player/{id}">{}</a>"#, escape(&peer.name)),
                None => escape(&peer.name),
            };
            format!(
                r#"<tr><td>{who}</td><td class="money {}">{}</td><td class="money">{}</td><td class="money">{}</td></tr>"#,
                if peer.net() >= 0 { "positive" } else { "negative" },
                signed_cents(peer.net()),
                format_cents(peer.sent),
                format_cents(peer.received),
            )
        })
        .collect::<String>();
    let blurb = if is_own_page {
        "What you have handed each person, less what they have handed you.".to_string()
    } else {
        format!(
            "What {} has handed each person, less what they have handed {}.",
            escape(name),
            escape(name)
        )
    };
    format!(
        r#"<section class="finance-panel gifts-panel"><h2>Gifts</h2><p>{blurb}</p><table class="finance-ledger gift-ledger"><thead><tr><th>Player</th><th>Net</th><th>Sent</th><th>Received</th></tr></thead><tbody>{rows}</tbody></table></section>"#
    )
}

fn finance_chart(account: &crate::bank::Account) -> String {
    let mut series = Vec::new();
    if let Some(first) = account.entries.first() {
        series.push((account.created_at, first.balance_after - first.delta));
    } else {
        series.push((account.created_at, account.balance));
    }
    series.extend(
        account
            .entries
            .iter()
            .map(|entry| (entry.at, entry.balance_after)),
    );
    let min = series
        .iter()
        .map(|(_, value)| *value)
        .min()
        .unwrap_or(0)
        .min(0);
    let max = series
        .iter()
        .map(|(_, value)| *value)
        .max()
        .unwrap_or(0)
        .max(0);
    let span = (max - min).max(1) as f64;
    let width = 640.0;
    let height = 220.0;
    let pad = 24.0;
    let denom = (series.len().saturating_sub(1)).max(1) as f64;
    let points = series
        .iter()
        .enumerate()
        .map(|(index, (_, value))| {
            let x = pad + (width - pad * 2.0) * index as f64 / denom;
            let y = height - pad - (height - pad * 2.0) * (*value - min) as f64 / span;
            format!("{x:.1},{y:.1}")
        })
        .collect::<Vec<_>>()
        .join(" ");
    let latest = series
        .last()
        .map(|(at, _)| at.format("%Y-%m-%d %H:%M UTC").to_string())
        .unwrap_or_else(|| "No entries yet".into());
    format!(
        r#"<figure class="finance-chart"><svg viewBox="0 0 640 220" role="img" aria-label="Player finances over time"><line class="axis" x1="24" y1="196" x2="616" y2="196"></line><line class="axis" x1="24" y1="24" x2="24" y2="196"></line><polyline points="{points}"></polyline></svg><figcaption><span>Low {}</span><span>High {}</span><span>Latest {}</span></figcaption></figure>"#,
        format_cents(min),
        format_cents(max),
        escape(&latest)
    )
}

fn signed_cents(value: Cents) -> String {
    if value >= 0 {
        format!("+{}", format_cents(value))
    } else {
        format!("-{}", format_cents(-value))
    }
}

pub fn table_page(view: &crate::view::TableView) -> String {
    let seats = view
        .seats
        .iter()
        .map(|seat| {
            format!(
                "<li>Seat {}: {} 🪙 {} — {}</li>",
                seat.index,
                escape(&seat.occupant),
                seat.bank_balance
                    .map(format_cents)
                    .unwrap_or_else(|| "—".into()),
                seat.stack
            )
        })
        .collect::<String>();
    let board = view
        .hand
        .as_ref()
        .map(|hand| {
            hand.board
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default();
    let app = format!("<div id=\"table-app\" data-table-id=\"{}\"></div>", view.id);
    let fallback = format!(
        "<noscript><section class=card><h1>{}</h1><p>Board: {}</p><p>Pot: {}</p><ul>{}</ul></section></noscript>{}",
        escape(&view.name),
        escape(&board),
        view.hand.as_ref().map(|hand| hand.pot).unwrap_or(0),
        seats,
        app
    );
    layout_with_header(
        &view.name,
        &fallback,
        &format!(
            r#"<script type="module" src="{}" defer></script>"#,
            asset("/public/table.js")
        ),
        Some(&view.name),
        r#"<button class="table-config-button" type="button" title="Card display settings" aria-label="Card display settings" commandfor="card-config" command="show-modal">⚙</button>"#,
    )
}

/// The winning-hand types the poker board breaks out.
///
/// High card is left off deliberately: it wins at showdown so rarely that a
/// column of dashes says less than the space costs. It still counts toward
/// `wins_shown`, so the shares stay honest.
const WINNING_CATEGORIES: [(crate::eval::Category, &str); 8] = [
    (crate::eval::Category::Pair, "Pair"),
    (crate::eval::Category::TwoPair, "2 pair"),
    (crate::eval::Category::ThreeOfAKind, "Trips"),
    (crate::eval::Category::Straight, "Str"),
    (crate::eval::Category::Flush, "Flush"),
    (crate::eval::Category::FullHouse, "Boat"),
    (crate::eval::Category::FourOfAKind, "Quads"),
    (crate::eval::Category::StraightFlush, "St fl"),
];

/// A tenths-of-a-percent share as a percentage, carrying the decimal only when
/// dropping it would change the number. Quads are rare enough that "0%" would
/// be a lie where "0.5%" is the truth, but a flat quarter should read "25%".
fn permille(value: u64) -> String {
    if value == 0 {
        String::new()
    } else if value.is_multiple_of(10) {
        format!("{}%", value / 10)
    } else {
        format!("{}.{}%", value / 10, value % 10)
    }
}

/// A player's name, linked to their page when they have one, with the house
/// marked as the house.
fn standings_name(name: &str, player_id: Option<Uuid>, house: bool) -> String {
    let linked = match player_id {
        Some(id) => format!(
            r#"<a class="player-link" href="/player/{id}">{}</a>"#,
            escape(name)
        ),
        None => escape(name),
    };
    format!(
        "{linked}{}",
        if house {
            " <i class=\"house-tag\">house</i>"
        } else {
            ""
        }
    )
}

/// The five cards of a made hand, as faces rather than text.
fn hand_faces(cards: &[crate::cards::Card]) -> String {
    cards
        .iter()
        .map(|card| {
            let text = card.to_string();
            card_face(&text[0..1], &text[1..2])
        })
        .collect()
}

fn short_date(at: chrono::DateTime<chrono::Utc>) -> String {
    at.format("%b %-d").to_string()
}

/// One board, with a heading and a note, or a line saying it is still empty.
fn standings_board(title: &str, note: &str, head: &str, body: &str, empty: &str) -> String {
    let table = if body.is_empty() {
        format!("<p class=\"standings-empty\">{}</p>", escape(empty))
    } else {
        format!(
            "<div class=\"standings-scroll\"><table class=\"leaderboard-table\">{head}<tbody>{body}</tbody></table></div>"
        )
    };
    format!(
        "<section class=\"standings-board\"><header><h2>{}</h2><p>{}</p></header>{table}</section>",
        escape(title),
        escape(note)
    )
}

/// Standings: the bankroll, then a board per game, then the record books.
///
/// This was one very wide table until the record books arrived; splitting it
/// per game is what let the winning-hand breakdown and the beat boards have
/// room for a real header instead of another `colspan`.
pub fn leaderboard(
    rows: &[crate::view::LeaderboardRow],
    straight_flushes: &[crate::view::LeaderboardBigHand],
    quads: &[crate::view::LeaderboardBigHand],
    bad_beats: &[crate::view::LeaderboardBeat],
    worst_beats: &[crate::view::LeaderboardBeat],
) -> String {
    // ---- the money, which is what ranks the page ----
    let money_body = rows
        .iter()
        .map(|row| {
            format!(
                "<tr><td class=\"rank\">{}</td><td>{}</td><td class=\"money\">{}</td><td class=\"money\">{}</td><td>{}</td></tr>",
                row.rank,
                standings_name(&row.name, row.player_id, row.house),
                format_cents(row.balance),
                format_cents(row.net_balance),
                row.loan_count,
            )
        })
        .collect::<String>();
    let money = standings_board(
        "Bankroll",
        "Net balance, house regulars included. A tie goes to whoever took fewer loans.",
        "<thead><tr><th></th><th>Player</th><th>Balance</th><th>Net</th><th>Loans</th></tr></thead>",
        &money_body,
        "Nobody has played yet.",
    );

    // ---- poker, with what people actually win with ----
    let poker_body = rows
        .iter()
        .filter(|row| row.poker.hands > 0)
        .map(|row| {
            let types = WINNING_CATEGORIES
                .iter()
                .map(|(category, _)| {
                    let won = row.poker.won_with(*category);
                    if won == 0 {
                        return "<td class=\"blitz-empty\">—</td>".to_string();
                    }
                    format!(
                        "<td>{won}<i class=\"of-wins\">{}</i></td>",
                        permille(row.poker.won_with_permille(*category))
                    )
                })
                .collect::<String>();
            format!(
                "<tr><td class=\"rank\">{}</td><td>{}</td><td>{}</td><td>{}%</td><td>{}%</td><td>{}</td><td>{}</td><td class=\"money\">{}</td>{types}</tr>",
                row.rank,
                standings_name(&row.name, row.player_id, row.house),
                row.poker.hands,
                row.poker.vpip_percent(),
                row.poker.pfr_percent(),
                row.poker.hands_won,
                row.poker.wins_shown,
                format_cents(row.poker.biggest_pot),
            )
        })
        .collect::<String>();
    let type_headers = WINNING_CATEGORIES
        .iter()
        .map(|(_, label)| format!("<th>{label}</th>"))
        .collect::<String>();
    let poker = standings_board(
        "Hold'em",
        "Each type column is winning hands of that make, over their share of the hands this player showed down. A hand everyone folded to never turns over, so the types add up to Shown, not to Won.",
        &format!(
            "<thead><tr><th></th><th></th><th></th><th></th><th></th><th></th><th></th><th></th><th colspan=\"{}\">Winning hands by type</th></tr><tr class=\"leaderboard-subhead\"><th></th><th>Player</th><th>Hands</th><th>VPIP</th><th>PFR</th><th>Won</th><th>Shown</th><th>Biggest pot</th>{type_headers}</tr></thead>",
            WINNING_CATEGORIES.len()
        ),
        &poker_body,
        "Nobody has played a hand yet.",
    );

    // ---- blackjack ----
    let blackjack_body = rows
        .iter()
        .filter(|row| row.blackjack.rounds > 0)
        .map(|row| {
            // The ledger knew the money and the result, never the detail, so a
            // record seeded from it says so rather than showing a false zero.
            let detail = if row.blackjack.watched() {
                format!(
                    "<td>{}</td><td>{}</td><td>{}</td><td>{}</td>",
                    row.blackjack.naturals,
                    row.blackjack.busts,
                    row.blackjack.doubles,
                    row.blackjack.splits,
                )
            } else {
                "<td class=\"blitz-empty\">—</td><td class=\"blitz-empty\">—</td><td class=\"blitz-empty\">—</td><td class=\"blitz-empty\">—</td>".to_string()
            };
            format!(
                "<tr><td class=\"rank\">{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}%</td>{detail}<td class=\"money\">{}</td><td class=\"money\">{}</td></tr>",
                row.rank,
                standings_name(&row.name, row.player_id, row.house),
                row.blackjack.rounds,
                row.blackjack.won,
                row.blackjack.lost,
                row.blackjack.push,
                row.blackjack.win_percent(),
                format_cents(row.blackjack.wagered),
                format_cents(row.blackjack.net()),
            )
        })
        .collect::<String>();
    let blackjack = standings_board(
        "Blackjack",
        "Every round on the books, counted back from the bank ledger. A dash means that player's record was read from the ledger, which knew the money and the result but never whether a hand busted, split or doubled — those fill in as they play.",
        "<thead><tr><th></th><th>Player</th><th>Rounds</th><th>Won</th><th>Lost</th><th>Push</th><th>Win</th><th>Naturals</th><th>Busts</th><th>Doubles</th><th>Splits</th><th>Wagered</th><th>Net</th></tr></thead>",
        &blackjack_body,
        "Nobody has sat down at the blackjack table yet.",
    );

    // ---- hand blitz ----
    let blitz_headers = rows.first().map_or_else(String::new, |row| {
        row.blitz
            .iter()
            .map(|blitz| format!("<th colspan=\"2\">{}</th>", escape(&blitz.difficulty)))
            .collect()
    });
    let blitz_subheaders = rows.first().map_or_else(String::new, |row| {
        row.blitz
            .iter()
            .map(|_| "<th>Accuracy</th><th>Streak</th>".to_string())
            .collect()
    });
    let blitz_body = rows
        .iter()
        .filter(|row| row.blitz.iter().any(|blitz| blitz.attempts > 0))
        .map(|row| {
            let cells = row
                .blitz
                .iter()
                .map(|blitz| {
                    if blitz.attempts == 0 {
                        return "<td class=\"blitz-empty\">—</td><td class=\"blitz-empty\">—</td>"
                            .to_string();
                    }
                    format!(
                        "<td>{}%</td><td>{}</td>",
                        blitz.accuracy_percent, blitz.best_streak
                    )
                })
                .collect::<String>();
            format!(
                "<tr><td class=\"rank\">{}</td><td>{}</td>{cells}</tr>",
                row.rank,
                standings_name(&row.name, row.player_id, row.house),
            )
        })
        .collect::<String>();
    let blitz = standings_board(
        "Hand Blitz",
        "How sharp people are at reading a board, which is a different kind of good.",
        &format!(
            "<thead><tr><th></th><th></th>{blitz_headers}</tr><tr class=\"leaderboard-subhead\"><th></th><th>Player</th>{blitz_subheaders}</tr></thead>"
        ),
        &blitz_body,
        "Nobody has played a run yet.",
    );

    // ---- the record books ----
    let big_hand_rows = |hands: &[crate::view::LeaderboardBigHand]| {
        hands
            .iter()
            .map(|hand| {
                format!(
                    "<tr><td class=\"rank\">{}</td><td>{}</td><td class=\"made-hand\">{}</td><td>{}{}</td><td class=\"money\">{}</td><td class=\"when\">{}</td></tr>",
                    hand.rank,
                    standings_name(&hand.name, hand.player_id, hand.house),
                    hand_faces(&hand.cards),
                    escape(&hand.label),
                    if hand.royal {
                        " <i class=\"royal-tag\">royal</i>"
                    } else {
                        ""
                    },
                    format_cents(hand.won),
                    escape(&short_date(hand.at)),
                )
            })
            .collect::<String>()
    };
    let big_hand_head = "<thead><tr><th></th><th>Player</th><th>Hand</th><th>Made</th><th>Won</th><th>When</th></tr></thead>";
    let straight_flush_board = standings_board(
        "Straight flushes",
        "Every one ever made. Rare enough that the board keeps them all rather than a top ten, and a royal is flagged where it falls.",
        big_hand_head,
        &big_hand_rows(straight_flushes),
        "Nobody has made one yet.",
    );
    let quads_board = standings_board(
        "Four of a kind",
        "The biggest by what the hand took down.",
        big_hand_head,
        &big_hand_rows(quads),
        "Nobody has made quads yet.",
    );

    // Both beat boards read the same row from opposite ends, so they share a
    // shape: who was ahead, who won anyway, and what it cost.
    let beat_rows = |beats: &[crate::view::LeaderboardBeat]| {
        beats
            .iter()
            .map(|beat| {
                format!(
                    "<tr><td class=\"rank\">{}</td><td>{}</td><td class=\"equity\"><span class=\"equity-bar\"><i style=\"width:{}%\"></i></span><b>{}</b></td><td class=\"made\">{}</td><td>{}</td><td class=\"made\">{}</td><td class=\"equity-plain\">{}</td><td class=\"money\">{}</td><td class=\"when\">{}</td></tr>",
                    beat.rank,
                    standings_name(&beat.loser, beat.loser_id, beat.loser_house),
                    beat.loser_equity_permille / 10,
                    permille(u64::from(beat.loser_equity_permille)),
                    escape(&beat.loser_label),
                    standings_name(&beat.winner, beat.winner_id, beat.winner_house),
                    escape(&beat.winner_label),
                    permille(u64::from(beat.winner_equity_permille)),
                    format_cents(beat.pot),
                    escape(&short_date(beat.at)),
                )
            })
            .collect::<String>()
    };
    let beat_head = "<thead><tr><th></th><th>Beaten</th><th>Their chance</th><th>Holding</th><th>Lost to</th><th>Who had</th><th>Their chance</th><th>Pot</th><th>When</th></tr></thead>";
    let bad_beats_board = standings_board(
        "Bad beats",
        "The biggest favourite who still lost, measured the moment the last chips went in and the board was not yet finished.",
        beat_head,
        &beat_rows(bad_beats),
        "Nobody has been beaten as a favourite yet.",
    );
    let worst_beats_board = standings_board(
        "Worst beats",
        &format!(
            "The costliest of them: the biggest pots lost while a {}% favourite or better.",
            crate::stats::COOLER_PERMILLE / 10
        ),
        beat_head,
        &beat_rows(worst_beats),
        "No pot has been lost from that far ahead yet.",
    );

    layout(
        "Leaderboard",
        &format!(
            "<section class=\"leaderboard\"><header class=\"history-top\"><div><h1>Leaderboard</h1><p>Top {} by net balance, house regulars included.</p></div><nav><a href=\"/tables\">Lobby</a> · <a href=\"/hand-blitz\">Hand Blitz</a></nav></header>{money}{poker}{blackjack}{blitz}{straight_flush_board}{quads_board}{bad_beats_board}{worst_beats_board}</section>",
            crate::routes::LEADERBOARD_SIZE
        ),
        "",
    )
}

/// The debugging view of a table's past hands, newest first.
pub fn table_history(
    id: Uuid,
    name: &str,
    total: usize,
    hands: &[crate::table::HandRecord],
    names: &std::collections::HashMap<usize, String>,
) -> String {
    // Two bots of the same kind are otherwise indistinguishable, so every
    // label carries its seat.
    let seat_label = |seat: usize, occupant: &crate::table::SeatOccupant| -> String {
        let who = names.get(&seat).cloned().unwrap_or_else(|| match occupant {
            crate::table::SeatOccupant::Bot { kind, seat } => {
                crate::table::Bot::new(*kind, *seat).name().to_string()
            }
            _ => "empty".to_string(),
        });
        format!("{seat} · {who}")
    };
    let rows = hands
        .iter()
        .rev()
        .map(|hand| {
            let seats = hand
                .seats
                .iter()
                .map(|seat| {
                    let awarded: crate::money::Cents = hand
                        .summary
                        .awards
                        .iter()
                        .filter(|award| award.seat == seat.seat)
                        .map(|award| award.amount)
                        .sum();
                    let result = hand
                        .summary
                        .results
                        .iter()
                        .find(|result| result.seat == seat.seat)
                        .and_then(|result| result.hand.as_ref())
                        .map_or_else(String::new, |ranked| escape(&ranked.label));
                    format!(
                        r#"<tr{}><td>{}{}</td><td class="cards">{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>"#,
                        if awarded > 0 { r#" class="won""# } else { "" },
                        escape(&seat_label(seat.seat, &seat.occupant)),
                        if hand.button == seat.seat { " (D)" } else { "" },
                        cards_html(&seat.hole_cards),
                        format_cents(seat.stack_before),
                        format_cents(seat.stack_after),
                        if awarded > 0 {
                            format_cents(awarded)
                        } else {
                            "—".into()
                        },
                        result
                    )
                })
                .collect::<String>();
            let actions = hand
                .summary
                .events
                .iter()
                .map(|event| {
                    format!(
                        "<li><span>{:?}</span><b>{}</b></li>",
                        event.street,
                        escape(&event_line(event, &hand.seats, &seat_label))
                    )
                })
                .collect::<String>();
            format!(
                r#"<details class="hand-record"><summary><b>Hand {}</b><span>{}</span><span class="cards">{}</span><span>{}</span></summary><table><thead><tr><th>Seat</th><th>Hole</th><th>Before</th><th>After</th><th>Won</th><th>Hand</th></tr></thead><tbody>{}</tbody></table><ol class="hand-actions">{}</ol></details>"#,
                hand.hand_no,
                hand.at.format("%Y-%m-%d %H:%M:%S UTC"),
                cards_html(&hand.summary.board),
                escape(&hand.stakes.to_string()),
                seats,
                actions
            )
        })
        .collect::<String>();
    let body = format!(
        r#"<section class="history-shell"><header class="history-top"><div><h1>{}</h1><p>{} hand{} recorded · showing the most recent {}</p></div><nav><a href="/tables/{}">Back to table</a> · <a href="/tables/{}/history?format=json" download>Download JSON</a></nav></header>{}</section>"#,
        escape(name),
        total,
        if total == 1 { "" } else { "s" },
        hands.len(),
        id,
        id,
        if rows.is_empty() {
            "<p class=\"loading\">No hands played yet.</p>".to_string()
        } else {
            rows
        }
    );
    layout(&format!("{name} history"), &body, "")
}

fn cards_html(cards: &[crate::cards::Card]) -> String {
    cards
        .iter()
        .map(|card| {
            let text = card.to_string();
            let red = text.ends_with('h') || text.ends_with('d');
            format!(
                r#"<i class="{}">{}</i>"#,
                if red { "card-red" } else { "card-black" },
                escape(&card_text(&text))
            )
        })
        .collect()
}

fn card_text(value: &str) -> String {
    let (rank, suit) = value.split_at(value.len() - 1);
    let rank = if rank == "T" { "10" } else { rank };
    let suit = match suit {
        "h" => "♥",
        "d" => "♦",
        "c" => "♣",
        "s" => "♠",
        other => other,
    };
    format!("{rank}{suit}")
}

fn event_line(
    event: &crate::holdem::HandEvent,
    seats: &[crate::table::HandRecordSeat],
    seat_label: &impl Fn(usize, &crate::table::SeatOccupant) -> String,
) -> String {
    let who = event.seat.map_or_else(String::new, |seat| {
        let occupant = seats
            .iter()
            .find(|entry| entry.seat == seat)
            .map_or(crate::table::SeatOccupant::Empty, |entry| {
                entry.occupant.clone()
            });
        seat_label(seat, &occupant)
    });
    let amount = if event.amount > 0 {
        format!(" {}", format_cents(event.amount))
    } else {
        String::new()
    };
    format!("{who} {:?}{amount}", event.kind)
}

fn card_face(rank: &str, suit: &str) -> String {
    let glyph = match suit {
        "h" => "♥",
        "d" => "♦",
        "c" => "♣",
        "s" => "♠",
        _ => "",
    };
    let display = if rank == "T" { "10" } else { rank };
    let color = if suit == "h" || suit == "d" {
        "red"
    } else {
        "black"
    };
    // Mirrors card.js: the face is rank over suit at one size.
    format!(
        r#"<span class="playing-card {color} suit-{suit}" aria-label="{rank}{suit}"><span class="card-corner"><b>{display}</b><i>{glyph}</i></span></span>"#
    )
}

fn suit_name(suit: &str) -> &'static str {
    match suit {
        "s" => "Spades",
        "h" => "Hearts",
        "d" => "Diamonds",
        "c" => "Clubs",
        _ => "",
    }
}

fn format_duration_ms(ms: u64) -> String {
    if ms == 0 {
        "—".into()
    } else {
        format!("{:.1}s", ms as f64 / 1_000.0)
    }
}

pub fn lobby(tables: &[crate::view::LobbyTableView], _balance: Cents) -> String {
    layout(
        "Lobby",
        &format!(
            "<section class=\"card lobby\"><h1>Lobby</h1>{}<p><a href=\"/hand-blitz\">Hand Blitz</a> · <a href=\"/blackjack\">Blackjack</a> · <a href=\"/leaderboard\">Leaderboard</a> · <a href=\"/tables/new\">Start a game</a></p></section>",
            lobby_table_list(tables, false)
        ),
        "",
    )
}

fn lobby_table_list(tables: &[crate::view::LobbyTableView], include_yours: bool) -> String {
    let row = |table: &crate::view::LobbyTableView| {
        let detail = if let Some(tournament) = &table.tournament {
            format!(
                "buy-in {} · {} · {}/{} seats",
                format_cents(tournament.buy_in),
                if tournament.registered == tournament.seat_count {
                    "running"
                } else {
                    "registering"
                },
                tournament.registered,
                tournament.seat_count
            )
        } else {
            format!(
                "{} buy-in · {} · {} · {}",
                format_cents(table.buy_in),
                table.stakes,
                match table.humans {
                    0 => "no humans".to_string(),
                    1 => "1 human".to_string(),
                    count => format!("{count} humans"),
                },
                format_args!("{}/{} seats", table.occupied, table.max_seats)
            )
        };
        format!(
            "<li><a href=\"/tables/{}\">{}</a><span>{}</span>{}</li>",
            table.id,
            escape(&table.name),
            detail,
            if table.your_seat.is_some() {
                " <b>Your seat</b>"
            } else {
                ""
            }
        )
    };
    let section = |title: &str, rows: &str, locked_title: &str, locked: &str, empty: &str| {
        format!(
            "<section class=\"table-list\"><h2>{title}</h2><ul>{}</ul>{}</section>",
            if rows.is_empty() { empty } else { rows },
            if locked.is_empty() {
                String::new()
            } else {
                format!(
                    "<details class=\"out-of-reach\"><summary>{locked_title}</summary><ul>{locked}</ul></details>"
                )
            }
        )
    };
    let mut yours = String::new();
    let mut cash = String::new();
    let mut cash_locked = String::new();
    let mut tournaments = String::new();
    let mut tournaments_locked = String::new();
    for table in tables {
        let entry = row(table);
        if include_yours && table.your_seat.is_some() {
            yours.push_str(&entry);
        } else if table.tournament.is_some() {
            if table.affordable {
                tournaments.push_str(&entry);
            } else {
                tournaments_locked.push_str(&entry);
            }
        } else if table.affordable {
            cash.push_str(&entry);
        } else {
            cash_locked.push_str(&entry);
        }
    }
    format!(
        "{}{}{}",
        if include_yours {
            section("Your seats", &yours, "", "", "<li>None yet</li>")
        } else {
            String::new()
        },
        section(
            "Cash tables",
            &cash,
            "Cash tables to spectate",
            &cash_locked,
            "<li>No tables yet</li>"
        ),
        section(
            "Tournaments",
            &tournaments,
            "Tournaments to spectate",
            &tournaments_locked,
            "<li>None running · <a href=\"/tables/new\">start one</a></li>"
        )
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::Category;

    fn blitz_boards() -> Vec<crate::view::LeaderboardBlitz> {
        crate::blitz::BlitzDifficulty::ALL
            .iter()
            .map(|difficulty| crate::view::LeaderboardBlitz {
                difficulty: difficulty.config().label.to_string(),
                attempts: 9,
                accuracy_percent: 88,
                best_streak: 7,
            })
            .collect()
    }

    fn row() -> crate::view::LeaderboardRow {
        let mut poker = crate::stats::PlayerStats {
            hands: 100,
            hands_won: 40,
            wins_shown: 20,
            ..Default::default()
        };
        poker.won_by_category.insert(Category::Flush, 5);
        poker.won_by_category.insert(Category::FourOfAKind, 1);
        crate::view::LeaderboardRow {
            rank: 1,
            name: "Dana".into(),
            player_id: None,
            house: false,
            balance: 1_000,
            net_balance: 1_000,
            loan_count: 0,
            poker,
            blackjack: crate::blackjack_stats::BlackjackStats {
                rounds: 4,
                hands: 4,
                won: 2,
                lost: 2,
                wagered: 400,
                returned: 400,
                ..Default::default()
            },
            blitz: blitz_boards(),
        }
    }

    #[test]
    fn every_blitz_difficulty_keeps_its_own_accuracy_and_streak_columns() {
        let html = leaderboard(&[row()], &[], &[], &[], &[]);
        for difficulty in crate::blitz::BlitzDifficulty::ALL {
            let label = difficulty.config().label;
            assert!(html.contains(label), "missing {label} columns");
        }
        assert_eq!(html.matches("<th>Accuracy</th><th>Streak</th>").count(), 3);
    }

    #[test]
    fn a_winning_hand_shows_its_count_over_its_share_of_the_hands_shown() {
        let html = leaderboard(&[row()], &[], &[], &[], &[]);
        // Five flushes out of twenty hands shown is a quarter of them.
        assert!(html.contains("<td>5<i class=\"of-wins\">25%</i></td>"));
        // One in twenty would round away to nothing as a whole percent, so
        // the rare makes keep a decimal.
        assert!(html.contains("<td>1<i class=\"of-wins\">5%</i></td>"));
        // A make nobody has won with is a dash, not a zero.
        assert!(html.contains("<td class=\"blitz-empty\">—</td>"));
    }

    #[test]
    fn a_record_seeded_from_the_ledger_admits_it_knows_no_detail() {
        // Nothing watched, so busts and splits are dashes rather than zeros.
        let mut seeded = row();
        seeded.blackjack.derived_rounds = seeded.blackjack.rounds;
        assert!(!seeded.blackjack.watched());
        let html = leaderboard(&[seeded], &[], &[], &[], &[]);
        let blackjack = html
            .split("Blackjack</h2>")
            .nth(1)
            .expect("a blackjack board");
        assert_eq!(blackjack.matches("class=\"blitz-empty\"").count(), 4);
    }

    #[test]
    fn a_royal_is_flagged_where_it_falls_and_a_beat_shows_both_ends() {
        let straight_flush = crate::view::LeaderboardBigHand {
            rank: 1,
            name: "Dana".into(),
            player_id: None,
            house: false,
            royal: true,
            label: "Straight flush, aces high".into(),
            cards: "AhKhQhJhTh"
                .as_bytes()
                .chunks(2)
                .map(|pair| std::str::from_utf8(pair).unwrap().parse().unwrap())
                .collect(),
            won: 100_000,
            at: chrono::Utc::now(),
        };
        let beat = crate::view::LeaderboardBeat {
            rank: 1,
            loser: "Scott".into(),
            loser_id: None,
            loser_house: false,
            loser_equity_permille: 977,
            loser_label: "Four of a kind, sevens".into(),
            winner: "Dana".into(),
            winner_id: None,
            winner_house: false,
            winner_equity_permille: 23,
            winner_label: "Straight flush, aces high".into(),
            pot: 100_000,
            at: chrono::Utc::now(),
        };
        let html = leaderboard(&[row()], &[straight_flush], &[], &[beat], &[]);
        assert!(html.contains("royal-tag"));
        // Five card faces for the hand that made it.
        assert!(html.contains("Straight flush, aces high"));
        // Both ends of the beat, and the favourite's share drawn as a bar.
        assert!(html.contains("Scott"));
        assert!(html.contains("Four of a kind, sevens"));
        assert!(html.contains("<b>97.7%</b>"));
        assert!(html.contains("width:97%"));
    }

    #[test]
    fn a_board_nobody_has_played_says_so_instead_of_showing_a_bare_header() {
        let html = leaderboard(&[], &[], &[], &[], &[]);
        assert!(html.contains("Nobody has played yet."));
        assert!(html.contains("Nobody has made one yet."));
        assert!(html.contains("No pot has been lost from that far ahead yet."));
    }
}
