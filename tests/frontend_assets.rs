const TABLE_JS: &str = include_str!("../public/table.js");
const APP_CSS: &str = include_str!("../public/app.css");
const BANK_JS: &str = include_str!("../public/bank.js");
const BLITZ_JS: &str = include_str!("../public/blitz.js");

#[test]
fn table_island_has_live_state_and_action_contracts() {
    for literal in [
        "new EventSource(`/tables/${tableId}/events`)",
        "fetchState()",
        "your_hole_cards",
        "legal_actions",
        "fold",
        "check",
        "call",
        "raise",
        "all_in",
        "Sit out",
        "Leave",
        "empty-seat",
        "Wager slider",
        "½ pot",
        "Showdown",
        "· bot",
        "Buy in for seat",
        "table-pot",
        "shared bot account",
        "award-list",
        "waiting-status",
        "/tables/${tableId}/bot",
        "/tournaments/${tableId}/register",
        "Seat a bot",
        "Register for tournament",
        "option value=\"fish\"",
        "table-error",
        "responseError",
    ] {
        assert!(
            TABLE_JS.contains(literal),
            "missing table.js contract: {literal}"
        );
    }
}

#[test]
fn table_css_is_mobile_poker_layout() {
    for literal in [
        ".felt",
        ".playing-card",
        ".card-corner",
        ".card-suit",
        ".seats",
        "@media(max-width:640px)",
        ".actions",
        ".table-center",
        ".empty-seat",
        ".table-pot",
        ".bank-widget[aria-expanded=true]",
        "position:absolute",
        "label[hidden]{display:none}",
    ] {
        assert!(
            APP_CSS.contains(literal),
            "missing app.css contract: {literal}"
        );
    }
}

#[test]
fn bank_widget_fetches_signed_in_balance() {
    assert!(BANK_JS.contains("fetch(\"/api/bank\""));
    assert!(BANK_JS.contains("bank-balance"));
    assert!(BANK_JS.contains("textContent"));
    assert!(BANK_JS.contains("bank-delta"));
    assert!(BANK_JS.contains("aria-expanded"));
}

#[test]
fn hand_blitz_island_has_mode_contracts() {
    for literal in [
        "/hand-blitz/start",
        "/hand-blitz/answer",
        "run_id",
        "round_id",
        "choice",
        "deadline_ms",
        "20s",
        "12s",
        "Correct:",
        "Play again",
    ] {
        assert!(
            BLITZ_JS.contains(literal),
            "missing blitz.js contract: {literal}"
        );
    }
    for literal in [
        ".blitz-shell",
        ".difficulty-grid",
        ".blitz-clock",
        ".blitz-hands",
    ] {
        assert!(
            APP_CSS.contains(literal),
            "missing app.css blitz contract: {literal}"
        );
    }
}
