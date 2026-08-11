use crate::money::format_cents;
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
pub fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
pub fn layout(title: &str, body: &str, head: &str) -> String {
    layout_with_context(title, body, head, None)
}

fn layout_with_context(title: &str, body: &str, head: &str, context: Option<&str>) -> String {
    let context = context.map_or_else(String::new, |value| {
        format!(r#"<span class="header-context">{}</span>"#, escape(value))
    });
    format!(
        r#"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>{}</title><link rel="stylesheet" href="{}">{}</head><body><main class="page"><header class="site-header"><a class="brand" href="/">♠ two-seven</a>{}<button class="bank-widget" type="button" title="Account balance" aria-expanded="false">🪙 <span id="bank-balance">—</span><span id="bank-delta"></span></button></header>{}</main><script type="module" src="{}" defer></script></body></html>"#,
        escape(title),
        asset("/public/app.css"),
        head,
        context,
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
                r#"<section class="card"><h1>Welcome, {}</h1><p>Play Texas Hold'em at a cash table.</p><p><a href="/tables">Open lobby</a> · <a href="/hand-blitz">Hand Blitz</a> · <a href="/blackjack">Blackjack</a> · <a href="/tables/new">Start a game</a></p><form method="post" action="/auth/logout"><button>Sign out</button></form></section>"#,
                escape(&name)
            ),
            "",
        ),
    }
}

pub fn home_lobby(name: &str, tables: &[crate::view::LobbyTableView]) -> String {
    layout(
        "Lobby",
        &format!(
            "<section class=\"card lobby\"><h1>Welcome, {}</h1>{}<p><a href=\"/hand-blitz\">Hand Blitz</a> · <a href=\"/blackjack\">Blackjack</a> · <a href=\"/tables/new\">Start a game</a></p><form method=\"post\" action=\"/auth/logout\"><button>Sign out</button></form></section>",
            escape(name),
            lobby_table_list(tables, true)
        ),
        "",
    )
}

pub fn table_create() -> String {
    game_create()
}

pub fn tournament_create() -> String {
    game_create()
}

fn game_create() -> String {
    layout(
        "Start a game",
        r#"<section class="setup-shell"><h1>Start a game</h1><form id="quick-game-form"><label>Game name<input name="name" required placeholder="Friday night"></label><fieldset class="setup-options"><legend>Setup</legend><label class="setup-option"><input type="radio" name="preset" value="cash-friendly" checked><span><b>Friendly cash</b><small>$1/$2 no-limit · $50–$200 · 6 seats</small></span></label><label class="setup-option"><input type="radio" name="preset" value="cash-standard"><span><b>Standard cash</b><small>$5/$10 no-limit · $250–$1,000 · 6 seats</small></span></label><label class="setup-option"><input type="radio" name="preset" value="cash-limit"><span><b>Limit cash</b><small>$10/$20 blinds · $20/$40 limit · 6 seats</small></span></label><label class="setup-option"><input type="radio" name="preset" value="tournament-quick"><span><b>Quick sit-and-go</b><small>$10 entry · 4 players · winner takes all</small></span></label><label class="setup-option"><input type="radio" name="preset" value="tournament-classic"><span><b>Classic sit-and-go</b><small>$50 entry · 6 players · top 2 paid</small></span></label><label class="setup-option"><input type="radio" name="preset" value="tournament-deep"><span><b>Deep-stack tournament</b><small>$200 entry · 9 players · top 3 paid</small></span></label></fieldset><label class="setup-debt"><input type="checkbox" name="no_debt"> Require available balance</label><button>Create game</button></form><p id="create-error" class="error" role="alert"></p><script type="module" src="/public/lobby.js"></script></section>"#,
        "",
    )
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
    layout(
        "Blackjack",
        r#"<section class="blitz-shell blackjack-shell"><div class="blitz-top"><div><h1>Blackjack</h1><p>Beat the dealer to 21. Blackjack pays 3:2.</p></div><a href="/tables">Lobby</a></div><div id="blackjack-app"><section class="blitz-menu"><form id="blackjack-form"><label>Bet ($)<input name="bet" type="number" min="1" max="10000" step="0.01" value="25.00"></label><button>Deal</button></form></section></div></section>"#,
        &format!(
            r#"<script type="module" src="{}" defer></script>"#,
            asset("/public/blackjack.js")
        ),
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
    layout_with_context(
        &view.name,
        &fallback,
        &format!(
            r#"<script type="module" src="{}" defer></script>"#,
            asset("/public/table.js")
        ),
        Some(&view.name),
    )
}

fn card_face(rank: &str, suit: &str) -> String {
    let pip = match suit {
        "h" => "♥",
        "d" => "♦",
        "c" => "♣",
        "s" => "♠",
        _ => "",
    };
    let display = if rank == "T" { "10" } else { rank };
    let value = match rank {
        "A" => 1,
        "K" => 13,
        "Q" => 12,
        "J" => 11,
        "T" => 10,
        _ => rank.parse::<usize>().unwrap_or(0),
    };
    let color = if suit == "h" || suit == "d" {
        "red"
    } else {
        "black"
    };
    let center = match value {
        1 => format!(
            r#"<span class="card-art card-art-A"><span class="ace-badge"><i>{}</i></span></span>"#,
            pip
        ),
        11..=13 => {
            let piece = match value {
                11 => "♘",
                12 => "♕",
                _ => "♔",
            };
            format!(
                r#"<span class="card-art card-art-{}"><span class="court-piece">{}</span><i>{}</i></span>"#,
                display, piece, pip
            )
        }
        _ => format!(
            r#"<span class="pip-grid pip-grid-{}">{}</span>"#,
            value,
            pip_positions(value)
                .iter()
                .map(|position| format!(r#"<i class="card-pip-{}">{}</i>"#, position, pip))
                .collect::<String>()
        ),
    };
    format!(
        r#"<span class="playing-card {}" aria-label="{}{}"><span class="card-corner"><b>{}</b><i>{}</i></span><span class="card-frame">{}</span><span class="card-corner card-corner-bottom"><b>{}</b><i>{}</i></span></span>"#,
        color, rank, suit, display, pip, center, display, pip
    )
}

fn pip_positions(value: usize) -> &'static [&'static str] {
    match value {
        2 => &["top-center", "bottom-center"],
        3 => &["top-center", "middle-center", "bottom-center"],
        4 => &["top-left", "top-right", "bottom-left", "bottom-right"],
        5 => &[
            "top-left",
            "top-right",
            "middle-center",
            "bottom-left",
            "bottom-right",
        ],
        6 => &[
            "top-left",
            "top-right",
            "middle-left",
            "middle-right",
            "bottom-left",
            "bottom-right",
        ],
        7 => &[
            "top-left",
            "top-right",
            "middle-left",
            "middle-right",
            "bottom-left",
            "bottom-right",
            "upper-center",
        ],
        8 => &[
            "top-left",
            "top-right",
            "middle-left",
            "middle-right",
            "bottom-left",
            "bottom-right",
            "upper-center",
            "lower-center",
        ],
        9 => &[
            "top-left",
            "top-right",
            "upper-left",
            "upper-right",
            "middle-center",
            "lower-left",
            "lower-right",
            "bottom-left",
            "bottom-right",
        ],
        10 => &[
            "top-left",
            "top-right",
            "upper-left",
            "upper-right",
            "middle-left",
            "middle-right",
            "lower-left",
            "lower-right",
            "bottom-left",
            "bottom-right",
        ],
        _ => &[],
    }
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

pub fn lobby(tables: &[crate::view::LobbyTableView]) -> String {
    layout(
        "Lobby",
        &format!(
            "<section class=\"card lobby\"><h1>Lobby</h1>{}<p><a href=\"/hand-blitz\">Hand Blitz</a> · <a href=\"/blackjack\">Blackjack</a> · <a href=\"/tables/new\">Start a game</a></p></section>",
            lobby_table_list(tables, false)
        ),
        "",
    )
}

fn lobby_table_list(tables: &[crate::view::LobbyTableView], include_yours: bool) -> String {
    let mut yours = String::new();
    let mut open = String::new();
    for table in tables {
        let is_yours = table.your_seat.is_some();
        let tournament = if let Some(tournament) = &table.tournament {
            format!(
                "Tournament · buy-in {} · {} · {}",
                format_cents(tournament.buy_in),
                if tournament.finished {
                    "finished"
                } else if tournament.registered == tournament.seat_count {
                    "running"
                } else {
                    "registering"
                },
                if tournament.paid_out {
                    "paid out".to_string()
                } else {
                    format!("{}/{} seats", tournament.registered, tournament.seat_count)
                }
            )
        } else {
            format!(
                "Cash · {} buy-in · {} · {} · {}/{} seats",
                format_cents(table.buy_in),
                table.stakes,
                if table.no_debt { "no-debt" } else { "standard" },
                table.occupied,
                table.max_seats
            )
        };
        let row = format!(
            "<li><a href=\"/tables/{}\">{}</a><span>{}</span>{}</li>",
            table.id,
            escape(&table.name),
            tournament,
            if is_yours { " <b>Your seat</b>" } else { "" }
        );
        if include_yours && is_yours {
            yours.push_str(&row);
        } else {
            open.push_str(&row);
        }
    }
    format!(
        "{}<section class=\"table-list\"><h2>{}</h2><ul>{}</ul></section>{}",
        if include_yours {
            format!(
                "<section class=\"table-list\"><h2>Your seats</h2><ul>{}</ul></section>",
                if yours.is_empty() {
                    "<li>None yet</li>".into()
                } else {
                    yours
                }
            )
        } else {
            String::new()
        },
        if include_yours {
            "Open tables"
        } else {
            "Tables"
        },
        if open.is_empty() {
            "<li>No tables yet</li>"
        } else {
            &open
        },
        ""
    )
}
