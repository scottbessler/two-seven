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
        r##"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1,viewport-fit=cover"><meta name="theme-color" content="#123d34"><meta name="apple-mobile-web-app-capable" content="yes"><meta name="apple-mobile-web-app-status-bar-style" content="default"><title>{}</title><link rel="manifest" href="{}"><link rel="icon" href="{}"><link rel="apple-touch-icon" href="{}"><link rel="stylesheet" href="{}">{}{}</head><body><main class="page"><header class="site-header"><a class="brand" href="/">♠ two-seven</a>{}<div class="bank-widget" role="button" tabindex="0" title="Account balance" aria-expanded="false">🪙 <span id="bank-balance">—</span><span id="bank-delta"></span></div></header>{}</main><script type="module" src="{}" defer></script></body></html>"##,
        escape(title),
        asset("/public/manifest.webmanifest"),
        asset("/public/icon.svg"),
        asset("/public/apple-touch-icon.svg"),
        asset("/public/app.css"),
        import_map(),
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
                r#"<section class="card"><h1>Welcome, {}</h1><p>Play Texas Hold'em at a cash table.</p><p><a href="/tables">Open lobby</a> · <a href="/hand-blitz">Hand Blitz</a> · <a href="/blackjack">Blackjack</a> · <a href="/tables/new">Start a game</a></p><form class="re-up-form"><button type="submit">Re-up $1,000</button></form><form method="post" action="/auth/logout"><button>Sign out</button></form></section>"#,
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
    // One question per step; lobby.js walks the steps and assembles the config.
    let step = |name: &str, legend: &str, options: &str| {
        format!(
            r#"<fieldset class="setup-step" data-step="{name}" hidden><legend>{legend}</legend><div class="setup-options">{options}</div></fieldset>"#
        )
    };
    let option = |name: &str, value: &str, title: &str, detail: &str| {
        format!(
            r#"<button class="setup-option" type="button" data-choice="{name}" value="{value}"><b>{title}</b><small>{detail}</small></button>"#
        )
    };
    let format_step = step(
        "format",
        "What are we playing?",
        &format!(
            "{}{}",
            option(
                "format",
                "cash",
                "Cash game",
                "Buy in, play any number of hands, cash out whenever"
            ),
            option(
                "format",
                "tournament",
                "Tournament",
                "One entry, 10,000 chips, play until someone has them all"
            )
        ),
    );
    let betting_step = step(
        "betting",
        "Which betting rules?",
        &format!(
            "{}{}",
            option(
                "betting",
                "no-limit",
                "No-limit",
                "Bet anything you have in front of you"
            ),
            option(
                "betting",
                "limit",
                "Limit",
                "Fixed bet sizes, a bet and three raises per street"
            )
        ),
    );
    let players_step = step(
        "players",
        "How many players?",
        &format!(
            "{}{}{}",
            option(
                "players",
                "4",
                "4 players",
                "Winner takes the whole prize pool"
            ),
            option("players", "6", "6 players", "Top 2 paid"),
            option("players", "9", "9 players", "Top 3 paid")
        ),
    );
    let buy_in_step = step(
        "buyIn",
        "How much to buy in?",
        &format!(
            "{}{}{}{}",
            option("buyIn", "20000", "$200", "$1/$2 blinds · $2/$4 limit"),
            option("buyIn", "50000", "$500", "$2/$4 blinds · $5/$10 limit"),
            option("buyIn", "100000", "$1,000", "$5/$10 blinds · $10/$20 limit"),
            option(
                "buyIn",
                "200000",
                "$2,000",
                "$10/$20 blinds · $20/$40 limit"
            )
        ),
    );
    let confirm_step = r#"<fieldset class="setup-step setup-confirm" data-step="confirm" hidden><legend>Name the game</legend><p class="setup-summary" id="setup-summary"></p><label>Game name<input name="name" required maxlength="48" placeholder="Friday night"></label><button class="setup-create" type="submit">Create game</button></fieldset>"#;
    let body = format!(
        r#"<section class="setup-shell"><dialog id="game-setup" class="setup-dialog"><form id="quick-game-form"><header><h2 id="setup-title">Start a game</h2><a class="setup-close" href="/tables" aria-label="Cancel">×</a></header>{format_step}{betting_step}{players_step}{buy_in_step}{confirm_step}<footer><button class="setup-back" type="button" hidden>Back</button><p id="create-error" class="error" role="alert"></p></footer></form></dialog><script type="module" src="{lobby}" defer></script></section>"#,
        lobby = asset("/public/lobby.js")
    );
    layout("Start a game", &body, "")
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
        r#"<section class="blitz-shell blackjack-shell"><div class="blitz-top"><div><h1>Blackjack</h1><p>Beat the dealer to 21. Blackjack pays 3:2.</p></div><a href="/tables">Lobby</a></div><div id="blackjack-app"><section class="blitz-table blackjack-table"><div class="actions blackjack-actions"><button class="deal-action" type="button">Deal $5</button><button class="deal-action" type="button">Deal $20</button><button class="deal-action" type="button">Deal $100</button><button class="deal-action" type="button">Deal $200</button></div></section></div></section>"#,
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
        r#"<span class="playing-card {color}" aria-label="{rank}{suit}"><span class="card-corner"><b>{display}</b><i>{glyph}</i></span></span>"#
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
                "Tournament · buy-in {} · {} · {}/{} seats",
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
