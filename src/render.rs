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
        r#"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>{}</title><link rel="stylesheet" href="{}">{}</head><body><main class="page"><header class="site-header"><a class="brand" href="/">♠ two-seven</a><a class="bank-widget" href="/api/bank" title="Account balance">🪙 <span id="bank-balance">—</span></a></header>{}</main><script src="{}" defer></script></body></html>"#,
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
                r#"<section class="card"><h1>Welcome, {}</h1><p>Play Texas Hold'em at a cash table.</p><p><a href="/tables">Browse tables</a> · <a href="/tables/new">Create a table</a> · <a href="/tournaments/new">Create a tournament</a></p><form method="post" action="/auth/logout"><button>Sign out</button></form></section>"#,
                escape(&name)
            ),
            "",
        ),
    }
}

pub fn table_create() -> String {
    layout(
        "New table",
        "<section class=\"card\"><h1>New table</h1><form id=\"create-table-form\"><input name=\"name\" required placeholder=\"Table name\"><select name=\"limit\"><option value=\"no-limit\">No-limit</option><option value=\"limit\">Limit</option></select><label><input type=\"checkbox\" name=\"no_debt\"> No-debt table</label><input name=\"min_buy_in\" type=\"number\" min=\"1\" value=\"1000\"><input name=\"max_buy_in\" type=\"number\" min=\"1\" value=\"10000\"><button>Create table</button></form><p id=\"create-error\" class=\"error\"></p><script type=\"module\" src=\"/public/lobby.js\"></script></section>",
        "",
    )
}

pub fn tournament_create() -> String {
    layout(
        "New tournament",
        r#"<section class="card"><h1>New sit-and-go</h1><form id="create-tournament-form"><input name="name" required placeholder="Tournament name"><input name="buy_in" type="number" min="1" value="1000"><input name="seat_count" type="number" min="2" max="9" value="4"><input name="starting_chips" type="number" min="1" value="1000"><label><input type="checkbox" name="no_debt"> No-debt registration</label><button>Create tournament</button></form><p id="create-error" class="error"></p><script type="module" src="/public/lobby.js"></script></section>"#,
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

pub fn lobby(
    _user: &Uuid,
    tables: &[(String, Uuid, crate::table::Stakes, usize, usize, bool)],
) -> String {
    let rows = tables
        .iter()
        .map(|(name, id, stakes, occupied, max, tournament)| {
            format!(
                "<li><a href=\"/tables/{id}\">{}</a> · {} · {:?} · {occupied}/{max} seats</li>",
                escape(name),
                if *tournament { "Tournament" } else { "Cash" },
                stakes
            )
        })
        .collect::<String>();
    layout(
        "Lobby",
        &format!(
            "<section class=card><h1>Lobby</h1><p><a href=\"/tables/new\">Create a cash table</a> · <a href=\"/tournaments/new\">Create a tournament</a></p><ul>{rows}</ul></section>"
        ),
        "",
    )
}
