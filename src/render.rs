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
        r#"<button class="sign-out-trigger" type="button">Sign out</button>"#,
        r#"<dialog id="sign-out" class="confirm-dialog"><div><header><h2>Sign out?</h2></header>"#,
        r#"<p>You will have to sign in again. Any table you are sitting at keeps your seat.</p>"#,
        r#"<footer><button class="sign-out-cancel" type="button">Stay signed in</button>"#,
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
                r#"<section class="card"><h1>Welcome, {}</h1><p>Play Texas Hold'em at a cash table.</p><p><a href="/tables">Open lobby</a> · <a href="/hand-blitz">Hand Blitz</a> · <a href="/blackjack">Blackjack</a> · <a href="/leaderboard">Leaderboard</a> · <a href="/tables/new">Start a game</a></p><form class="re-up-form"><button type="submit">Re-up $1,000</button></form>{}</section>"#,
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
            "<section class=\"card lobby\"><h1>Welcome, {}</h1>{}<p><a href=\"/hand-blitz\">Hand Blitz</a> · <a href=\"/blackjack\">Blackjack</a> · <a href=\"/leaderboard\">Leaderboard</a> · <a href=\"/tables/new\">Start a game</a></p>{}</section>",
            escape(name),
            lobby_table_list(tables, true),
            sign_out()
        ),
        "",
    )
}

pub fn table_create(_balance: Cents) -> String {
    game_create()
}

pub fn tournament_create(_balance: Cents) -> String {
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
    let buy_ins = [
        (20_000, "$200"),
        (50_000, "$500"),
        (100_000, "$1,000"),
        (200_000, "$2,000"),
        (500_000, "$5,000"),
        (1_000_000, "$10,000"),
    ];
    let buy_in_step = step(
        "buyIn",
        "How much to buy in?",
        &buy_ins
            .iter()
            .map(|(amount, label)| {
                option(
                    "buyIn",
                    &amount.to_string(),
                    label,
                    "10,000 chips · blinds climb every few hands",
                )
            })
            .collect::<String>(),
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
    layout(
        "Blackjack",
        r#"<section class="blitz-shell blackjack-shell"><div class="blitz-top"><div><h1>Blackjack</h1><p>Beat the dealer to 21. Blackjack pays 3:2.</p></div><a href="/tables">Lobby</a></div><div id="blackjack-app"><section class="blitz-table blackjack-table"><div class="actions blackjack-actions"><span class="deal-broke">Loading your stakes…</span></div></section></div></section>"#,
        &format!(
            r#"<script type="module" src="{}" defer></script>"#,
            asset("/public/blackjack.js")
        ),
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
            r#"<section class="card admin-panel"><h1>Admin</h1>{notice}{error}<form method="post" action="/admin"><label>Secret password<input type="password" name="password" autocomplete="current-password" required autofocus></label><div class="admin-actions"><button class="danger" type="submit" name="action" value="money">Reset all money and loans</button><button class="danger" type="submit" name="action" value="poker">Reset all poker stats</button><button class="danger" type="submit" name="action" value="blitz">Reset all blitz stats</button></div></form><p><a href="/tables">Lobby</a></p></section>"#
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

/// Standings: the bankroll, and how well people read a board.
pub fn leaderboard(rows: &[crate::view::LeaderboardRow]) -> String {
    let headers = rows.first().map_or_else(String::new, |row| {
        row.blitz
            .iter()
            .map(|blitz| format!("<th colspan=\"2\">{}</th>", escape(&blitz.difficulty)))
            .collect()
    });
    let subheaders = rows.first().map_or_else(String::new, |row| {
        row.blitz
            .iter()
            .map(|_| "<th>Accuracy</th><th>Streak</th>".to_string())
            .collect()
    });
    let body = rows
        .iter()
        .map(|row| {
            let blitz = row
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
                "<tr><td class=\"rank\">{}</td><td>{}{}</td><td class=\"money\">{}</td><td>{}</td><td>{}</td><td>{}%</td><td>{}%</td><td>{}%</td><td class=\"money\">{}</td>{}</tr>",
                row.rank,
                escape(&row.name),
                if row.house { " <i class=\"house-tag\">house</i>" } else { "" },
                format_cents(row.balance),
                row.loan_count,
                row.poker.hands,
                row.poker.vpip_percent(),
                row.poker.pfr_percent(),
                row.poker.win_percent(),
                format_cents(row.poker.biggest_pot),
                blitz
            )
        })
        .collect::<String>();
    let table = if rows.is_empty() {
        "<p class=\"loading\">Nobody has played yet.</p>".to_string()
    } else {
        format!(
            "<table class=\"leaderboard-table\"><thead><tr><th></th><th>Player</th><th>Balance</th><th>Loans</th><th colspan=\"5\">Poker</th>{headers}</tr><tr class=\"leaderboard-subhead\"><th></th><th></th><th></th><th></th><th>Hands</th><th>VPIP</th><th>PFR</th><th>Won</th><th>Biggest pot</th>{subheaders}</tr></thead><tbody>{body}</tbody></table>"
        )
    };
    layout(
        "Leaderboard",
        &format!(
            "<section class=\"leaderboard\"><header class=\"history-top\"><div><h1>Leaderboard</h1><p>Top {} by balance, house players included. A tie goes to whoever took fewer loans.</p></div><nav><a href=\"/tables\">Lobby</a> · <a href=\"/hand-blitz\">Hand Blitz</a></nav></header>{table}</section>",
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
    let section = |title: &str, rows: &str, empty: &str| {
        format!(
            "<section class=\"table-list\"><h2>{title}</h2><ul>{}</ul></section>",
            if rows.is_empty() { empty } else { rows }
        )
    };
    let mut yours = String::new();
    let mut cash = String::new();
    let mut tournaments = String::new();
    for table in tables {
        let entry = row(table);
        if include_yours && table.your_seat.is_some() {
            yours.push_str(&entry);
        } else if table.tournament.is_some() {
            tournaments.push_str(&entry);
        } else {
            cash.push_str(&entry);
        }
    }
    format!(
        "{}{}{}",
        if include_yours {
            section("Your seats", &yours, "<li>None yet</li>")
        } else {
            String::new()
        },
        section("Cash tables", &cash, "<li>No tables yet</li>"),
        section(
            "Tournaments",
            &tournaments,
            "<li>None running · <a href=\"/tables/new\">start one</a></li>"
        )
    )
}
