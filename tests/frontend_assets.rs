const TABLE_JS: &str = include_str!("../public/table.js");
const APP_RS: &str = include_str!("../src/app.rs");
const ACTIVE_CSS: &str = concat!(
    include_str!("../public/css/01-tokens.css"),
    include_str!("../public/css/02-base.css"),
    include_str!("../public/css/03-controls.css"),
    include_str!("../public/css/04-cards.css"),
    include_str!("../public/css/05-table.css"),
    include_str!("../public/css/06-blackjack.css"),
    include_str!("../public/css/07-pages.css"),
);
const CSS_SOURCES: [(&str, &str); 7] = [
    ("01-tokens.css", include_str!("../public/css/01-tokens.css")),
    ("02-base.css", include_str!("../public/css/02-base.css")),
    (
        "03-controls.css",
        include_str!("../public/css/03-controls.css"),
    ),
    ("04-cards.css", include_str!("../public/css/04-cards.css")),
    ("05-table.css", include_str!("../public/css/05-table.css")),
    (
        "06-blackjack.css",
        include_str!("../public/css/06-blackjack.css"),
    ),
    ("07-pages.css", include_str!("../public/css/07-pages.css")),
];
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

/// Assets normally get their version from `asset()`. A font can't: the
/// `@font-face` src lives in static CSS that no template touches, so the
/// version has to be in the filename (`bitter-v42-latin.woff2`) and both
/// references must be spelled the same way to stay one cached file.
fn is_filename_versioned(url: &str) -> bool {
    url.rsplit('/').next().is_some_and(|name| {
        name.split('-')
            .any(|part| part.starts_with('v') && part[1..].chars().all(|c| c.is_ascii_digit()))
    })
}

#[test]
fn rendered_public_assets_are_versioned() {
    for attribute in [r#"src="/public/"#, r#"href="/public/"#] {
        for (index, _) in RENDER_RS.match_indices(attribute) {
            let value = &RENDER_RS[index + attribute.len() - "/public/".len()..];
            let url = &value[..value.find('"').expect("attribute should be closed")];
            assert!(
                is_filename_versioned(url),
                "{url} is rendered unversioned: use asset() or put the version in the filename"
            );
        }
    }
}

#[test]
fn preloaded_font_matches_the_font_face_it_preloads() {
    let start = RENDER_RS
        .find(r#"<link rel="preload" href=""#)
        .expect("render.rs should preload the UI font");
    let url = &RENDER_RS[start + r#"<link rel="preload" href=""#.len()..];
    let url = &url[..url.find('"').expect("preload href should be closed")];
    assert!(
        ACTIVE_CSS.contains(&format!("url({url})")),
        "preloaded {url} is not the URL any @font-face requests, so the preload is wasted"
    );
    assert!(
        RENDER_RS[start..start + 200].contains("crossorigin"),
        "font preloads must be CORS-mode to match the font fetch"
    );
}

fn contains_raw_color_literal(css: &str) -> bool {
    css.match_indices('#').any(|(index, _)| {
        let value = &css[index + 1..];
        let digits = value.chars().take_while(char::is_ascii_hexdigit).count();
        (3..=8).contains(&digits)
            && value[digits..]
                .chars()
                .next()
                .is_none_or(|character| !character.is_ascii_hexdigit())
    })
}

fn contains_raw_border_radius_literal(css: &str) -> bool {
    css.match_indices("border-radius:").any(|(index, _)| {
        css[index..]
            .split_once(';')
            .map_or(&css[index..], |(declaration, _)| declaration)
            .contains("px")
    })
}

#[test]
fn split_css_assets_are_rendered_and_versioned() {
    for path in [
        "/public/css/01-tokens.css",
        "/public/css/02-base.css",
        "/public/css/03-controls.css",
        "/public/css/04-cards.css",
        "/public/css/05-table.css",
        "/public/css/06-blackjack.css",
        "/public/css/07-pages.css",
    ] {
        let filesystem_path = path.trim_start_matches('/');
        assert!(
            RENDER_RS.contains(&format!("asset(\"{path}\")")),
            "render.rs should version {path}"
        );
        assert!(
            APP_RS.contains(&format!("\"{filesystem_path}\"")),
            "app.rs should hash {path}"
        );
    }
}

#[test]
fn css_tokens_are_structurally_isolated() {
    let root_files: Vec<_> = CSS_SOURCES
        .iter()
        .filter(|(_, css)| css.contains(":root"))
        .map(|(name, _)| *name)
        .collect();
    assert_eq!(root_files, vec!["01-tokens.css"]);

    for (name, css) in CSS_SOURCES.iter().skip(1) {
        assert!(
            !contains_raw_color_literal(css),
            "{name} should not contain raw color literals"
        );
        assert!(
            !contains_raw_border_radius_literal(css),
            "{name} should use radius tokens instead of raw pixel literals"
        );
    }

    // Suit colors are scoped through `--suit-ink`, so nothing needs `!important`.
    for (name, css) in CSS_SOURCES {
        assert!(
            !css.contains("!important"),
            "{name} should not need !important"
        );
    }
}

/// The e2e harness renders production's own Bitter and only supplies fallbacks
/// for the glyphs it lacks (tests/e2e/rendering.ts), so a change to this token
/// desynchronises the two. Pin it here; the harness names Bitter directly.
#[test]
fn production_font_stacks_are_deliberate() {
    let tokens = CSS_SOURCES[0].1;
    let base = CSS_SOURCES[1].1;
    let cards = CSS_SOURCES[3].1;
    assert!(
        tokens.contains(r#"--font-ui:"Bitter",ui-serif,Georgia,"Times New Roman",serif"#),
        "UI text is self-hosted Bitter, so an installed PWA keeps its type offline"
    );
    assert!(
        !tokens.contains("--font-card"),
        "labels and card faces share one family; a second token invites them to drift"
    );
    assert!(base.contains("font:16px var(--font-ui)"));
    assert!(cards.contains("font-family:var(--font-ui)"));
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
        "HoldAction",
        "Hold ${ariaLabel || label} for ${holdSeconds} second",
        "action-middle",
        "settings.confirmFold",
        "settings.confirmAllIn",
        "Leave",
        "Leaving...",
        "Re-Buy In",
        "player-tooltip",
        "seat-cards",
        "wagerOptions",
        "table-stage",
        "showdown-result",
        "ShowdownOdds",
        "showdown-odds",
        "equity_permille",
        "reveal_odds",
        "showdown-advance",
        "/tables/${tableId}/continue",
        "winner",
        "Buy In",
        "status-log",
        "/tables/${tableId}/bot",
        "/tournaments/${tableId}/register",
        "Seat ${kind}",
        "\"shark\"",
        "table-error",
        "responseError",
        "rawRank === \"T\" ? \"10\"",
        "card-corner rank over suit",
        "response.ok",
        "hand.legal_actions.to_call",
        "state.tournament.started",
        "state.tournament?.finished && (!showdown || settled)",
        "header-info",
        "street_contribution",
        "SmallBlind",
        "BigBlind",
        "Current bet",
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
        ".four-color-suits",
        ".seats",
        "@media(max-width:640px)",
        ".actions",
        ".action-middle",
        ".hold-action",
        ".table-center",
        ".player-tooltip",
        ".seat-cards",
        ".card-config-dialog",
        ".card-config-preview",
        ".table-config-button",
        "--viewer-card-w",
        ".table-stage .card-zoom-target:not(.empty-card):hover",
        ".table-stage",
        ".showdown-result",
        ".showdown-odds",
        "flex-wrap:nowrap",
        "overflow-x:auto",
        ".table-center:has(.showdown-odds)",
        ".showdown-progress",
        ".winner-role",
        ".table-metrics",
        ".game-log",
        ".seat.acting",
        ".seat.folded",
        ".seat-wager",
        ".bank-widget[open]",
        ".player-page",
        ".finance-chart",
        ".finance-ledger",
        "position:absolute",
        "label[hidden]{display:none}",
    ] {
        assert!(
            ACTIVE_CSS.contains(literal),
            "missing active CSS contract: {literal}"
        );
    }
}

#[test]
fn bank_widget_fetches_signed_in_balance() {
    assert!(BANK_JS.contains("fetch(\"/api/bank\""));
    assert!(BANK_JS.contains("bank-balance"));
    assert!(BANK_JS.contains("/player"));
    assert!(BANK_JS.contains("player-page-link"));
    assert!(BANK_JS.contains("textContent"));
    assert!(BANK_JS.contains("bank-delta"));
    assert!(BANK_JS.contains("netChangeInLastHour"));
    assert!(BANK_JS.contains("60 * 60 * 1000"));
    assert!(BANK_JS.contains("bank-panel"));
    assert!(BANK_JS.contains("bank:updated"));
    assert!(BANK_JS.contains("can_re_up"));
    assert!(BANK_JS.contains("document.addEventListener(\"click\""));
    assert!(BANK_JS.contains("event.key === \"Escape\""));
    assert!(!BANK_JS.contains("aria-expanded"));
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
        "card-corner rank over suit",
        "Correct:",
        "Play again",
        "refreshBank",
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
            ACTIVE_CSS.contains(literal),
            "missing active CSS blitz contract: {literal}"
        );
    }
}

#[test]
fn shared_card_renderer_draws_rank_over_suit_only() {
    assert!(CARD_JS.contains("card-corner"));
    assert!(CARD_JS.contains("<b>${face.rank}</b><i>${face.suit}</i>"));
    assert!(CARD_JS.contains("suit-${face.suitCode}"));
    assert!(RENDER_RS.contains("suit-{suit}"));
    assert!(CARD_JS.contains("card-back"));
    assert!(CARD_JS.contains("empty-card"));
    for dropped in [
        "pip-grid",
        "card-art",
        "court-piece",
        "ace-badge",
        "card-frame",
    ] {
        assert!(
            !CARD_JS.contains(dropped),
            "the card face no longer renders {dropped}"
        );
    }
}

#[test]
fn shared_card_settings_preserve_storage_contract() {
    for literal in [
        "table-card-size-percent",
        "table-rank-size-percent",
        "table-rank-weight-percent",
        "table-four-color-suits",
        "table-paranoid-cards",
        "table-confirm-fold",
        "table-confirm-all-in",
        "localStorage",
        "Card display",
        "card-config-dialog",
        "card-config-preview",
        "--viewer-card-scale",
        "--viewer-card-w",
        "--card-rank-weight",
        "--card-rank-stroke",
        "four-color-suits",
        "Hold to fold",
        "Hold for all-in",
        r#"command="show-modal""#,
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
        "card-corner rank over suit",
        "deal-action",
        "betOptions",
        "Deal ${wholeDollarMoney(amount)}",
        "bank:updated",
        "game.can_hit && html",
        "game.can_double && html",
        "actions blackjack-actions",
        "TRAINER_KEYS",
        "blackjack-trainer-decks",
        "blackjack-trainer-penetration-percent",
        "blackjack-penetration-percent",
        "penetration_percent",
        "counting_tutor",
        "counting_quiz",
        "bet_analyzer",
        "blackjack-trainer-count",
        "blackjack-trainer-log",
        "blackjack-quiz",
        "blackjack-analysis",
        "blackjack-status-row",
        "blackjack-shoe",
        "blackjack-shoe-bar",
        "blackjack-shoe-marker",
        "blackjack-shoe-text",
        "Fresh shuffle",
        "trigger=${false}",
    ] {
        assert!(
            BLACKJACK_JS.contains(literal),
            "missing blackjack.js contract: {literal}"
        );
    }
    for literal in [
        ".blackjack-trainer-count",
        ".blackjack-trainer-log",
        ".blackjack-quiz",
        ".blackjack-analysis",
        ".blackjack-play-area",
        "justify-content:center",
        ".blackjack-play-area .blackjack-hand:first-child",
        ".blackjack-play-area .blackjack-hand:nth-child(2):last-child",
        ".blackjack-status-row",
        ".blackjack-shoe",
        ".blackjack-shoe-bar",
        ".blackjack-shoe-marker",
        ".blackjack-shoe-text",
    ] {
        assert!(
            ACTIVE_CSS.contains(literal),
            "missing blackjack trainer CSS contract: {literal}"
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
        "export function announceBank",
        "export async function refreshBank",
        "bank:updated",
        "CustomEvent",
    ] {
        assert!(
            SHARED_JS.contains(literal),
            "missing shared.js contract: {literal}"
        );
    }
    assert!(BLITZ_JS.contains("responseError(response)"));
}

#[test]
fn setup_walks_a_stepped_game_dialog() {
    for literal in [
        "players",
        "buyIn",
        "confirm",
        "endpoint: \"/tournaments\"",
        "starting_chips: TOURNAMENT_CHIPS",
        "payout_percentages: PAYOUTS[seats]",
        "players * 2",
        "showModal()",
    ] {
        assert!(
            LOBBY_JS.contains(literal),
            "missing setup contract: {literal}"
        );
    }
    // Cash games are standing tables now; only tournaments are created here.
    assert!(!LOBBY_JS.contains("/tables\""));
    assert!(LOBBY_JS.contains("TOURNAMENT_CHIPS = 1_000_000"));
    assert_eq!(LOBBY_JS.matches("  [").count(), 15, "T10,000 has 15 levels");
    assert!(TABLE_JS.contains("money(state.buy_in)"));
    assert!(TABLE_JS.contains("body: \"{}\""));
    assert!(TABLE_JS.contains("refreshBank"));
    assert!(TABLE_JS.contains("bank:updated"));
    assert!(TABLE_JS.contains("JSON.stringify({ kind })"));
    assert!(!TABLE_JS.contains("JSON.stringify({ seat })"));
}
