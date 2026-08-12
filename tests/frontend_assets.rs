const TABLE_JS: &str = include_str!("../public/table.js");
const APP_CSS: &str = include_str!("../public/app.css");
const BANK_JS: &str = include_str!("../public/bank.js");
const BLACKJACK_JS: &str = include_str!("../public/blackjack.js");
const BLITZ_JS: &str = include_str!("../public/blitz.js");
const CARD_JS: &str = include_str!("../public/card.js");
const SHARED_JS: &str = include_str!("../public/shared.js");
const LOBBY_JS: &str = include_str!("../public/lobby.js");
const CARD_SETTINGS_JS: &str = include_str!("../public/card-settings.js");
const RENDER_RS: &str = include_str!("../src/render.rs");

#[test]
fn imported_islands_are_emitted_as_module_scripts() {
    let bank_asset = RENDER_RS
        .find(r#"asset("/public/bank.js")"#)
        .expect("render.rs should include bank.js");
    let script = &RENDER_RS[..bank_asset];
    let script = &script[script.rfind("<script").expect("bank script tag")..];
    assert!(
        script.starts_with(r#"<script type="module" src="{}" defer></script>"#),
        "bank.js must be emitted as a module script"
    );
}

#[test]
fn rendered_public_assets_are_versioned() {
    assert!(!RENDER_RS.contains(r#"src="/public/"#));
    assert!(!RENDER_RS.contains(r#"href="/public/"#));
}

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
        "Leave",
        "Leaving...",
        "Re-Buy In",
        "player-tooltip",
        "seat-cards",
        "wagerOptions",
        "table-stage",
        "showdown-result",
        "showdown-advance",
        "/tables/${tableId}/continue",
        "winner",
        "Buy In",
        "waiting-status",
        "/tables/${tableId}/bot",
        "/tournaments/${tableId}/register",
        "Seat a bot",
        "option value=\"fish\"",
        "table-error",
        "responseError",
        "rawRank === \"T\" ? \"10\"",
        "pip-grid-${value}",
        "card-pip-${position}",
        "card-art-${court}",
        "response.ok",
        "hand.legal_actions.to_call",
        "state.tournament.started",
        "Table log",
        "street_contribution",
        "SmallBlind",
        "BigBlind",
        "Current bet",
        "acting-role",
        "seat-wager",
    ] {
        assert!(
            TABLE_JS.contains(literal),
            "missing table.js contract: {literal}"
        );
    }
    assert!(!TABLE_JS.contains("Wager slider"));
    assert!(!TABLE_JS.contains("type=\"number\" min=${wager.min}"));
    assert!(!TABLE_JS.contains("Sit out"));
    assert!(!TABLE_JS.contains("/tables/${tableId}/sit"));
    assert!(!TABLE_JS.contains("sitting_out: true"));
}

#[test]
fn table_css_is_mobile_poker_layout() {
    for literal in [
        ".felt",
        ".playing-card",
        ".card-corner",
        ".pip-grid",
        ".card-art",
        ".seats",
        "@media(max-width:640px)",
        ".actions",
        ".table-center",
        ".player-tooltip",
        ".seat-cards",
        ".card-config-dialog",
        ".card-config-preview",
        ".table-config-button",
        "--viewer-card-w",
        ".table-stage .card-zoom-target:not(.empty-card):hover",
        "border-radius:40px",
        ".table-stage",
        ".showdown-result",
        ".showdown-progress",
        ".seat.winner",
        ".table-pot",
        ".table-metrics",
        ".game-log",
        ".seat.acting",
        ".seat.folded",
        ".seat-wager",
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
        "/hand-blitz/resume",
        "/hand-blitz/answer",
        "run_id",
        "round_id",
        "choice",
        "deadline_ms",
        "20s",
        "12s",
        "rawRank === \"T\" ? \"10\"",
        "pip-grid-${numeric}",
        "card-pip-${position}",
        "card-art-${court}",
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

#[test]
fn shared_card_renderer_has_inner_frame_contract() {
    assert!(CARD_JS.contains("card-frame"));
    assert!(CARD_JS.contains("pip-grid-${face.numeric}"));
    assert!(CARD_JS.contains("ace-badge"));
    assert!(CARD_JS.contains("court-piece"));
}

#[test]
fn shared_card_settings_preserve_storage_contract() {
    for literal in [
        "table-card-size-percent",
        "table-rank-size-percent",
        "table-rank-weight-percent",
        "localStorage",
        "Card display",
        "card-config-dialog",
        "card-config-preview",
        "--viewer-card-scale",
        "--viewer-card-w",
        "--card-rank-weight",
    ] {
        assert!(
            CARD_SETTINGS_JS.contains(literal),
            "missing card settings contract: {literal}"
        );
    }
}

#[test]
fn blackjack_island_has_game_contracts() {
    for literal in [
        "/blackjack/start",
        "/blackjack/${kind}",
        "/blackjack/resume",
        "act(\"hit\")",
        "act(\"stand\")",
        "dealer_score",
        "can_hit",
        "can_stand",
        "Dealer",
        "card-pip-${position}",
        "game.can_hit && html",
        "game.can_double && html",
        "actions blackjack-actions",
    ] {
        assert!(
            BLACKJACK_JS.contains(literal),
            "missing blackjack.js contract: {literal}"
        );
    }
}

#[test]
fn shared_island_helpers_have_contracts() {
    for literal in [
        "export function cents",
        "export function money",
        "export function wholeDollarMoney",
        "export async function responseError",
    ] {
        assert!(
            SHARED_JS.contains(literal),
            "missing shared.js contract: {literal}"
        );
    }
    assert!(BLITZ_JS.contains("responseError(response)"));
}

#[test]
fn setup_uses_six_bounded_presets() {
    for preset in [
        "cash-friendly",
        "cash-standard",
        "cash-limit",
        "tournament-quick",
        "tournament-classic",
        "tournament-deep",
    ] {
        assert!(LOBBY_JS.contains(preset), "missing setup preset: {preset}");
    }
    assert!(LOBBY_JS.contains("buy_in: 200_000"));
    assert!(LOBBY_JS.contains("buy_in: 20_000"));
    assert!(LOBBY_JS.contains("endpoint: \"/tables\""));
    assert!(LOBBY_JS.contains("endpoint: \"/tournaments\""));
    assert!(TABLE_JS.contains("money(state.buy_in)"));
    assert!(TABLE_JS.contains("body: \"{}\""));
    assert!(!TABLE_JS.contains("JSON.stringify({ seat })"));
}
