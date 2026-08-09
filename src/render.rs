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
    format!(
        r#"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>{}</title><link rel="stylesheet" href="{}">{}</head><body><main class="page"><header class="site-header"><a class="brand" href="/">♠ two-seven</a><button class="bank-widget" type="button" title="Account balance" aria-expanded="false">🪙 <span id="bank-balance">—</span><span id="bank-delta"></span></button></header>{}</main><script src="{}" defer></script></body></html>"#,
        escape(title),
        asset("/public/app.css"),
        head,
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
                r#"<section class="card"><h1>Welcome, {}</h1><p>Play Texas Hold'em at a cash table.</p><p><a href="/tables">Open lobby</a> · <a href="/tables/new">Create a table</a> · <a href="/tournaments/new">Create a tournament</a></p><form method="post" action="/auth/logout"><button>Sign out</button></form></section>"#,
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
            "<section class=\"card lobby\"><h1>Welcome, {}</h1>{}<p><a href=\"/tables/new\">Create a cash table</a> · <a href=\"/tournaments/new\">Create a tournament</a></p><form method=\"post\" action=\"/auth/logout\"><button>Sign out</button></form></section>",
            escape(name),
            lobby_table_list(tables, true)
        ),
        "",
    )
}

pub fn table_create() -> String {
    layout(
        "New table",
        "<section class=\"card\"><h1>New table</h1><form id=\"create-table-form\"><label>Table name<input name=\"name\" required placeholder=\"Table name\"></label><label>Game<select name=\"limit\"><option value=\"no-limit\">No-limit</option><option value=\"limit\">Limit</option></select></label><div class=\"stakes-fields\"><label>Small blind ($)<input name=\"small_blind\" type=\"number\" min=\"0.01\" step=\"0.01\" value=\"1.00\"></label><label>Big blind ($)<input name=\"big_blind\" type=\"number\" min=\"0.02\" step=\"0.01\" value=\"2.00\"></label><label>Small bet ($)<input name=\"small_bet\" type=\"number\" min=\"0.01\" step=\"0.01\" value=\"2.00\"></label><label>Big bet ($)<input name=\"big_bet\" type=\"number\" min=\"0.02\" step=\"0.01\" value=\"4.00\"></label></div><label>Minimum buy-in ($)<input name=\"min_buy_in\" type=\"number\" min=\"0.01\" step=\"0.01\" value=\"10.00\"></label><label>Maximum buy-in ($)<input name=\"max_buy_in\" type=\"number\" min=\"0.01\" step=\"0.01\" value=\"100.00\"></label><label><input type=\"checkbox\" name=\"no_debt\"> No-debt table</label><button>Create table</button></form><p id=\"create-error\" class=\"error\"></p><script type=\"module\" src=\"/public/lobby.js\"></script></section>",
        "",
    )
}

pub fn tournament_create() -> String {
    layout(
        "New tournament",
        r#"<section class="card"><h1>New sit-and-go</h1><form id="create-tournament-form"><label>Tournament name<input name="name" required placeholder="Tournament name"></label><label>Buy-in ($)<input name="buy_in" type="number" min="0.01" step="0.01" value="10.00"></label><label>Players<input name="seat_count" type="number" min="2" max="9" value="4"></label><label>Starting chips<input name="starting_chips" type="number" min="1" value="1000"></label><fieldset><legend>Blind schedule</legend><div id="levels"><label>Small blind ($)<input name="small_blind_1" type="number" step="0.01" value="0.10"></label><label>Big blind ($)<input name="big_blind_1" type="number" step="0.01" value="0.20"></label><label>Ante ($)<input name="ante_1" type="number" step="0.01" value="0"></label><label>Hands<input name="hands_1" type="number" min="1" value="10"></label><label>Small blind ($)<input name="small_blind_2" type="number" step="0.01" value="0.20"></label><label>Big blind ($)<input name="big_blind_2" type="number" step="0.01" value="0.40"></label><label>Ante ($)<input name="ante_2" type="number" step="0.01" value="0.05"></label><label>Hands<input name="hands_2" type="number" min="1" value="10"></label></div></fieldset><label>Payout percentages (comma-separated)<input name="payouts" value="65,35"></label><label><input type="checkbox" name="no_debt"> No-debt registration</label><button>Create tournament</button></form><p id="create-error" class="error"></p><script type="module" src="/public/lobby.js"></script></section>"#,
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
    layout(
        &view.name,
        &fallback,
        &format!(
            r#"<script type="module" src="{}" defer></script>"#,
            asset("/public/table.js")
        ),
    )
}

pub fn lobby(tables: &[crate::view::LobbyTableView]) -> String {
    layout(
        "Lobby",
        &format!(
            "<section class=\"card lobby\"><h1>Lobby</h1>{}<p><a href=\"/tables/new\">Create a cash table</a> · <a href=\"/tournaments/new\">Create a tournament</a></p></section>",
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
                "Cash · {} · {} · {}/{} seats",
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
