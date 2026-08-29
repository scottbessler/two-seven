//! Bot-vs-bot benchmark: seats bots at a table, plays out many hands with
//! stacks topped up to 100bb each hand, and prints per-bot stats.
//!
//! Usage: `cargo run --release --bin bot_bench -- [hands] [seed] [lineup] [stakes]`
//! where `lineup` is a comma-separated list of bot kinds, e.g.
//! `fish,rock,grinder,shark` (the default). Shark also accepts named
//! parameter presets as `shark:<preset>`; `shark:default` is an alias for
//! `shark`. Bench-only `steal` and `steal_check` bots are also available.
//! `stakes` is optional and accepts `no-limit` (the default) or `limit`.

use std::collections::BTreeMap;
use std::str::FromStr;

use two_seven::{
    bot::{SharkFrequency, SharkParams, SharkRatio, shark_with},
    holdem::{Action, Hand, HandEventKind, LegalActions, Street},
    table::{BotKind, Stakes},
    view::{HandView, hand_view},
};

#[derive(Default)]
struct SeatStats {
    hands: u64,
    net: i64,
    vpip_hands: u64,
    pfr_hands: u64,
    saw_flop: u64,
    showdowns: u64,
    showdown_wins: u64,
    hand_wins: u64,
    bets: u64,
    raises: u64,
    calls: u64,
    checks: u64,
    folds: u64,
    all_ins: u64,
}

impl SeatStats {
    fn record(&mut self, action: Action) {
        match action {
            Action::Bet { .. } => self.bets += 1,
            Action::Raise { .. } => self.raises += 1,
            Action::Call => self.calls += 1,
            Action::Check => self.checks += 1,
            Action::Fold => self.folds += 1,
            Action::AllIn => self.all_ins += 1,
        }
    }
}

enum BenchBot {
    Kind(BotKind),
    Shark {
        label: String,
        params: Box<SharkParams>,
    },
    Steal {
        check_when_free: bool,
    },
}

impl BenchBot {
    fn act(&self, view: &HandView, legal: &LegalActions, seed: u64) -> Action {
        match self {
            Self::Kind(kind) => kind.act(view, legal, seed),
            Self::Shark { params, .. } => shark_with(params, view, legal, seed),
            Self::Steal { check_when_free } => steal_action(view, legal, *check_when_free),
        }
    }

    fn label(&self) -> String {
        match self {
            Self::Kind(kind) => kind.to_string(),
            Self::Shark { label, .. } => label.clone(),
            Self::Steal { check_when_free } => {
                if *check_when_free {
                    "steal_check".into()
                } else {
                    "steal".into()
                }
            }
        }
    }
}

fn shark_preset(name: &str) -> Option<SharkParams> {
    match name {
        "default" => Some(SharkParams::DEFAULT),
        "phase1" => Some(phase1_params()),
        "conservative" => Some(conservative_params()),
        "nit" => Some(nit_params()),
        "aggro" => Some(aggro_params()),
        "features" => Some(features_params()),
        "aggro_noprobe" => Some(aggro_noprobe_params()),
        "tuned" => Some(tuned_params()),
        "samples25" => Some(samples25_params()),
        "samples50" => Some(samples50_params()),
        "samples200" => Some(samples200_params()),
        "samples400" => Some(samples400_params()),
        "samples64" => Some(samples64_params()),
        _ => None,
    }
}

fn phase1_params() -> SharkParams {
    let mut params = SharkParams::DEFAULT;
    apply_conservative_thresholds(&mut params);
    let off = SharkFrequency {
        numerator: 0,
        denominator: 1,
    };
    params.strong_draw_semi_bluff_frequency = off;
    params.weak_draw_semi_bluff_frequency = off;
    params.draw_semi_bluff_max_opponents = 0;
    params.draw_semi_bluff_out_of_position = false;
    params.implied_odds_equity_cap = 0.0;
    params.polarized_value_ratio = params.value_bet_ratio;
    params.polarized_value_equity = 1.0;
    params.polarized_value_frequency = off;
    params.thin_value_ratio = params.value_bet_ratio;
    params.probe_frequency = off;
    params.passive_value_edge_discount = 0.0;
    params.current_street_aggression_edge_premium = 0.0;
    params.aggressive_bettor_call_equity_premium = 0.0;
    params
}

fn apply_conservative_thresholds(params: &mut SharkParams) {
    params.late_open_score = 4;
    params.middle_open_score = 5;
    params.early_open_score = 6;
    params.big_blind_defense_score = 3;
    params.small_blind_defense_score = 4;
    params.other_defense_score = 6;
    params.heads_up_in_position_edge = 0.10;
    params.heads_up_out_of_position_edge = 0.16;
    params.multiway_in_position_edge = 0.14;
    params.multiway_out_of_position_edge = 0.20;
}

fn conservative_params() -> SharkParams {
    let mut params = SharkParams::DEFAULT;
    apply_conservative_thresholds(&mut params);
    params
}

fn nit_params() -> SharkParams {
    let mut params = conservative_params();
    params.late_open_score = 5;
    params.middle_open_score = 6;
    params.early_open_score = 7;
    params.big_blind_defense_score = 4;
    params.small_blind_defense_score = 5;
    params.other_defense_score = 7;
    params.heads_up_in_position_edge = 0.12;
    params.heads_up_out_of_position_edge = 0.18;
    params.multiway_in_position_edge = 0.16;
    params.multiway_out_of_position_edge = 0.22;
    params.strong_draw_semi_bluff_frequency = SharkFrequency {
        numerator: 1,
        denominator: 6,
    };
    params.weak_draw_semi_bluff_frequency = SharkFrequency {
        numerator: 1,
        denominator: 8,
    };
    params.probe_frequency = SharkFrequency {
        numerator: 0,
        denominator: 1,
    };
    params
}

fn apply_aggro_features(params: &mut SharkParams) {
    params.strong_draw_semi_bluff_frequency = SharkFrequency {
        numerator: 4,
        denominator: 5,
    };
    params.weak_draw_semi_bluff_frequency = SharkFrequency {
        numerator: 1,
        denominator: 2,
    };
    params.implied_odds_equity_cap = 0.15;
    params.probe_frequency = SharkFrequency {
        numerator: 1,
        denominator: 3,
    };
    params.polarized_value_ratio = SharkRatio::new(5, 4);
    params.value_bet_ratio = SharkRatio::new(3, 4);
    params.thin_value_ratio = SharkRatio::new(2, 3);
    params.semi_bluff_ratio = SharkRatio::new(2, 3);
}

fn aggro_params() -> SharkParams {
    let mut params = SharkParams::DEFAULT;
    apply_aggro_features(&mut params);
    params
}

fn features_params() -> SharkParams {
    let mut params = conservative_params();
    apply_aggro_features(&mut params);
    params
}

fn aggro_noprobe_params() -> SharkParams {
    let mut params = aggro_params();
    params.probe_frequency = SharkFrequency {
        numerator: 0,
        denominator: 1,
    };
    params
}

fn tuned_params() -> SharkParams {
    let mut params = aggro_params();
    params.implied_odds_equity_cap = SharkParams::DEFAULT.implied_odds_equity_cap;
    params.implied_odds_stack_pot_ratio = SharkParams::DEFAULT.implied_odds_stack_pot_ratio;
    params.polarized_value_ratio = SharkParams::DEFAULT.polarized_value_ratio;
    params.value_bet_ratio = SharkParams::DEFAULT.value_bet_ratio;
    params.thin_value_ratio = SharkParams::DEFAULT.thin_value_ratio;
    params.semi_bluff_ratio = SharkParams::DEFAULT.semi_bluff_ratio;
    params.thin_value_edge_cap = SharkParams::DEFAULT.thin_value_edge_cap;
    params
}

fn sample_params(flop: usize, turn: usize, river: usize) -> SharkParams {
    let mut params = SharkParams::DEFAULT;
    params.flop_samples = flop;
    params.turn_samples = turn;
    params.river_samples = river;
    params
}

fn samples25_params() -> SharkParams {
    sample_params(40, 56, 80)
}

fn samples50_params() -> SharkParams {
    sample_params(80, 112, 160)
}

fn samples200_params() -> SharkParams {
    sample_params(320, 448, 640)
}

fn samples400_params() -> SharkParams {
    sample_params(640, 896, 1280)
}

fn samples64_params() -> SharkParams {
    sample_params(64, 64, 64)
}

fn preflop_is_unraised(view: &HandView) -> bool {
    if !view.board.is_empty() {
        return false;
    }
    let mut contributions = BTreeMap::new();
    for event in view
        .events
        .iter()
        .filter(|event| event.street == Street::Preflop)
    {
        let Some(seat) = event.seat else {
            continue;
        };
        let before = contributions.get(&seat).copied().unwrap_or(0);
        let after = before + event.amount;
        match event.kind {
            HandEventKind::Bet | HandEventKind::Raise => return false,
            HandEventKind::AllIn => {
                let max_other = contributions
                    .iter()
                    .filter(|(other_seat, _)| **other_seat != seat)
                    .map(|(_, contribution)| *contribution)
                    .max()
                    .unwrap_or(0);
                if after > max_other {
                    return false;
                }
            }
            HandEventKind::Ante
            | HandEventKind::SmallBlind
            | HandEventKind::BigBlind
            | HandEventKind::Call
            | HandEventKind::Check
            | HandEventKind::Fold
            | HandEventKind::Deal
            | HandEventKind::Award => {}
        }
        contributions.insert(seat, after);
    }
    true
}

fn pot_raise_action(view: &HandView, legal: &LegalActions) -> Option<Action> {
    let bounds = legal.wager?;
    let amount = (legal.to_call + view.pot).clamp(bounds.min, bounds.max);
    legal.actions.iter().find_map(|action| match action {
        Action::Bet { .. } => Some(Action::Bet { amount }),
        Action::Raise { .. } => Some(Action::Raise { amount }),
        _ => None,
    })
}

fn free_action_or_fold(legal: &LegalActions) -> Action {
    if legal.to_call == 0 {
        if let Some(action) = legal
            .actions
            .iter()
            .copied()
            .find(|action| matches!(action, Action::Call))
        {
            return action;
        }
        if let Some(action) = legal
            .actions
            .iter()
            .copied()
            .find(|action| matches!(action, Action::Check))
        {
            return action;
        }
    }
    legal
        .actions
        .iter()
        .copied()
        .find(|action| matches!(action, Action::Fold))
        .unwrap_or(Action::Fold)
}

fn fold_only(legal: &LegalActions) -> Action {
    legal
        .actions
        .iter()
        .copied()
        .find(|action| matches!(action, Action::Fold))
        .unwrap_or(Action::Fold)
}

fn postflop_give_up(legal: &LegalActions, check_when_free: bool) -> Action {
    if check_when_free {
        if let Some(action) = legal
            .actions
            .iter()
            .copied()
            .find(|action| matches!(action, Action::Check))
        {
            return action;
        }
    } else if let Some(action) = legal
        .actions
        .iter()
        .copied()
        .find(|action| matches!(action, Action::Fold))
    {
        return action;
    }
    if let Some(action) = legal
        .actions
        .iter()
        .copied()
        .find(|action| matches!(action, Action::Fold | Action::Check))
    {
        return action;
    }
    Action::Fold
}

fn steal_action(view: &HandView, legal: &LegalActions, check_when_free: bool) -> Action {
    if !view.board.is_empty() {
        return postflop_give_up(legal, check_when_free);
    }
    if !preflop_is_unraised(view) {
        return fold_only(legal);
    }
    pot_raise_action(view, legal).unwrap_or_else(|| free_action_or_fold(legal))
}

fn parse_kind(name: &str) -> BenchBot {
    let normalized = name.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "fish" => BenchBot::Kind(BotKind::Fish),
        "rock" => BenchBot::Kind(BotKind::Rock),
        "grinder" => BenchBot::Kind(BotKind::Grinder),
        "shark" => BenchBot::Kind(BotKind::Shark),
        "steal" => BenchBot::Steal {
            check_when_free: false,
        },
        "steal_check" => BenchBot::Steal {
            check_when_free: true,
        },
        _ => {
            let Some(preset) = normalized.strip_prefix("shark:") else {
                panic!("unknown bot kind: {normalized}");
            };
            let Some(params) = shark_preset(preset) else {
                panic!("unknown Shark preset: {preset}");
            };
            BenchBot::Shark {
                label: normalized,
                params: Box::new(params),
            }
        }
    }
}

fn parse_stakes(name: &str) -> Stakes {
    match name.trim().to_ascii_lowercase().as_str() {
        "no-limit" | "no_limit" | "nolimit" => Stakes::NoLimit {
            small_blind: 100,
            big_blind: 200,
        },
        "limit" | "fixed-limit" | "fixed_limit" | "fixedlimit" => Stakes::Limit {
            small_bet: 200,
            big_bet: 400,
        },
        _ => panic!("stakes must be limit or no-limit"),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let hands: u64 = args
        .first()
        .map(|value| u64::from_str(value).expect("hands must be a number"))
        .unwrap_or(10_000);
    let base_seed: u64 = args
        .get(1)
        .map(|value| u64::from_str(value).expect("seed must be a number"))
        .unwrap_or(27);
    let lineup: Vec<BenchBot> = args
        .get(2)
        .map(|value| value.split(',').map(parse_kind).collect())
        .unwrap_or_else(|| {
            vec![
                BenchBot::Kind(BotKind::Fish),
                BenchBot::Kind(BotKind::Rock),
                BenchBot::Kind(BotKind::Grinder),
                BenchBot::Kind(BotKind::Shark),
            ]
        });
    assert!(
        (2..=9).contains(&lineup.len()),
        "lineup must have 2-9 seats"
    );
    let labels = display_labels(&lineup);

    let stakes = args
        .get(3)
        .map_or_else(|| parse_stakes("no-limit"), |value| parse_stakes(value));
    let big_blind = stakes.blinds().1;
    let buy_in = big_blind * 100;
    let stacks: Vec<i64> = vec![buy_in; lineup.len()];
    let mut stats: Vec<SeatStats> = lineup.iter().map(|_| SeatStats::default()).collect();

    for hand_index in 0..hands {
        let seed = base_seed.wrapping_add(hand_index.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let button = (hand_index as usize) % lineup.len();
        let mut hand = Hand::new(stakes, &stacks, button, seed);
        let mut vpip = vec![false; lineup.len()];
        let mut pfr = vec![false; lineup.len()];
        let mut turn = 0u64;
        while !hand.complete {
            let legal = hand.legal_actions().expect("a player should be to act");
            let seat = legal.seat;
            let view = hand_view(&hand, Some(seat), &[]);
            let action = lineup[seat].act(&view, &legal, seed ^ (turn << 8) ^ seat as u64);
            if hand.street == Street::Preflop {
                match action {
                    Action::Call | Action::Bet { .. } | Action::Raise { .. } | Action::AllIn => {
                        vpip[seat] = true;
                    }
                    _ => {}
                }
                if matches!(action, Action::Raise { .. } | Action::Bet { .. }) {
                    pfr[seat] = true;
                }
            }
            stats[seat].record(action);
            hand.apply_action(action)
                .unwrap_or_else(|error| panic!("hand {hand_index} turn {turn}: {error}"));
            turn += 1;
        }

        let summary = hand.summary.as_ref().expect("completed hand has summary");
        let mut awards: BTreeMap<usize, i64> = BTreeMap::new();
        for award in &summary.awards {
            *awards.entry(award.seat).or_default() += award.amount;
        }
        let showdown_seats: Vec<usize> = summary
            .results
            .iter()
            .filter(|result| result.hand.is_some())
            .map(|result| result.seat)
            .collect();
        for (seat, seat_stats) in stats.iter_mut().enumerate() {
            seat_stats.hands += 1;
            let contributed = summary.contributions.get(&seat).copied().unwrap_or(0);
            let won = awards.get(&seat).copied().unwrap_or(0);
            seat_stats.net += won - contributed;
            if vpip[seat] {
                seat_stats.vpip_hands += 1;
            }
            if pfr[seat] {
                seat_stats.pfr_hands += 1;
            }
            if won > 0 {
                seat_stats.hand_wins += 1;
            }
            if showdown_seats.contains(&seat) {
                seat_stats.showdowns += 1;
                if won > 0 {
                    seat_stats.showdown_wins += 1;
                }
            }
        }
        for player in &hand.players {
            if !player.hole_cards.is_empty() && hand.board.len() >= 3 && !player.folded {
                stats[player.seat].saw_flop += 1;
            }
        }
    }

    println!(
        "{hands} hands, {}-handed, stakes {stakes} (100bb stacks, topped up each hand), seed {base_seed}",
        lineup.len(),
    );
    println!();
    println!(
        "| bot | net | bb/100 | win% | vpip% | pfr% | wtsd% | w$sd% | bet | raise | call | check | fold | AF |"
    );
    println!("|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|");
    for (seat, label) in labels.iter().enumerate() {
        let s = &stats[seat];
        let hands_f = s.hands as f64;
        let pct = |count: u64| 100.0 * count as f64 / hands_f;
        let aggression = (s.bets + s.raises + s.all_ins) as f64 / (s.calls.max(1)) as f64;
        println!(
            "| {} | {} | {:.1} | {:.1}% | {:.1}% | {:.1}% | {:.1}% | {:.1}% | {} | {} | {} | {} | {} | {:.2} |",
            label,
            s.net,
            100.0 * s.net as f64 / big_blind as f64 / hands_f,
            pct(s.hand_wins),
            pct(s.vpip_hands),
            pct(s.pfr_hands),
            pct(s.showdowns),
            if s.showdowns > 0 {
                100.0 * s.showdown_wins as f64 / s.showdowns as f64
            } else {
                0.0
            },
            s.bets,
            s.raises,
            s.calls,
            s.checks,
            s.folds,
            aggression
        );
    }
}

fn display_labels(lineup: &[BenchBot]) -> Vec<String> {
    let mut counts = BTreeMap::new();
    for bot in lineup {
        *counts.entry(bot.label()).or_insert(0usize) += 1;
    }
    lineup
        .iter()
        .enumerate()
        .map(|(seat, bot)| {
            let label = bot.label();
            if counts[&label] > 1 {
                format!("{label} (seat {seat})")
            } else {
                label
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shark_defaults_and_alias_have_distinct_labels() {
        let default = parse_kind("shark");
        let alias = parse_kind("shark:default");
        assert_eq!(default.label(), "shark");
        assert_eq!(alias.label(), "shark:default");
        assert!(matches!(default, BenchBot::Kind(BotKind::Shark)));
        match alias {
            BenchBot::Shark { params, .. } => assert_eq!(*params, SharkParams::DEFAULT),
            BenchBot::Kind(_) => panic!("expected Shark preset"),
            BenchBot::Steal { .. } => panic!("expected Shark preset"),
        }
    }

    #[test]
    fn shark_strategy_presets_are_registered() {
        for name in [
            "phase1",
            "conservative",
            "nit",
            "aggro",
            "features",
            "aggro_noprobe",
            "tuned",
            "samples25",
            "samples50",
            "samples200",
            "samples400",
            "samples64",
        ] {
            let parsed = parse_kind(&format!("shark:{name}"));
            assert_eq!(parsed.label(), format!("shark:{name}"));
            assert!(matches!(parsed, BenchBot::Shark { .. }));
        }
    }

    #[test]
    fn steal_bots_are_registered() {
        assert_eq!(parse_kind("steal").label(), "steal");
        assert_eq!(parse_kind("steal_check").label(), "steal_check");
    }

    #[test]
    fn steal_bots_follow_their_preflop_rules() {
        let view = HandView {
            street: "Preflop".into(),
            button: 0,
            big_blind: 2,
            board: Vec::new(),
            your_hole_cards: None,
            seats: Vec::new(),
            pot: 300,
            current_player: Some(0),
            legal_actions: None,
            summary: None,
            players: Vec::new(),
            events: Vec::new(),
            last_bet: 200,
            to_call: 200,
            awaiting_advance: false,
            runout_leaders: Vec::new(),
            runout_odds: Vec::new(),
        };
        let legal = LegalActions {
            seat: 0,
            actions: vec![Action::Fold, Action::Call, Action::Raise { amount: 400 }],
            to_call: 200,
            wager: Some(two_seven::holdem::WagerBounds {
                min: 400,
                max: 1_000,
                fixed: None,
            }),
            wagers_capped: false,
        };
        assert_eq!(
            steal_action(&view, &legal, false),
            Action::Raise { amount: 500 }
        );

        let fixed_limit_legal = LegalActions {
            wager: Some(two_seven::holdem::WagerBounds {
                min: 400,
                max: 400,
                fixed: Some(400),
            }),
            ..legal.clone()
        };
        assert_eq!(
            steal_action(&view, &fixed_limit_legal, false),
            Action::Raise { amount: 400 }
        );

        let limped_view = HandView {
            pot: 700,
            events: vec![
                two_seven::holdem::HandEvent {
                    street: Street::Preflop,
                    seat: Some(1),
                    kind: HandEventKind::Call,
                    amount: 200,
                },
                two_seven::holdem::HandEvent {
                    street: Street::Preflop,
                    seat: Some(2),
                    kind: HandEventKind::Call,
                    amount: 200,
                },
            ],
            ..view.clone()
        };
        let limped_legal = LegalActions {
            wager: Some(two_seven::holdem::WagerBounds {
                min: 400,
                max: 1_000,
                fixed: None,
            }),
            ..legal.clone()
        };
        assert_eq!(
            steal_action(&limped_view, &limped_legal, false),
            Action::Raise { amount: 900 }
        );

        let raised_view = HandView {
            events: vec![two_seven::holdem::HandEvent {
                street: Street::Preflop,
                seat: Some(1),
                kind: HandEventKind::Raise,
                amount: 400,
            }],
            ..view.clone()
        };
        assert_eq!(steal_action(&raised_view, &legal, false), Action::Fold);

        let limp_then_raise_view = HandView {
            events: vec![
                two_seven::holdem::HandEvent {
                    street: Street::Preflop,
                    seat: Some(1),
                    kind: HandEventKind::Call,
                    amount: 200,
                },
                two_seven::holdem::HandEvent {
                    street: Street::Preflop,
                    seat: Some(2),
                    kind: HandEventKind::Raise,
                    amount: 400,
                },
            ],
            ..view
        };
        assert_eq!(
            steal_action(&limp_then_raise_view, &legal, false),
            Action::Fold
        );
    }

    #[test]
    fn steal_bots_give_up_postflop_differently() {
        let view = HandView {
            board: vec![two_seven::cards::Card::new(
                two_seven::cards::Rank::Ace,
                two_seven::cards::Suit::Spades,
            )],
            ..HandView {
                street: "Flop".into(),
                button: 0,
                big_blind: 2,
                board: Vec::new(),
                your_hole_cards: None,
                seats: Vec::new(),
                pot: 300,
                current_player: Some(0),
                legal_actions: None,
                summary: None,
                players: Vec::new(),
                events: Vec::new(),
                last_bet: 0,
                to_call: 0,
                awaiting_advance: false,
                runout_leaders: Vec::new(),
                runout_odds: Vec::new(),
            }
        };
        let legal = LegalActions {
            seat: 0,
            actions: vec![Action::Fold, Action::Check],
            to_call: 0,
            wager: None,
            wagers_capped: false,
        };
        assert_eq!(steal_action(&view, &legal, false), Action::Fold);
        assert_eq!(steal_action(&view, &legal, true), Action::Check);

        let facing_bet = LegalActions {
            actions: vec![Action::Fold, Action::Call],
            to_call: 100,
            ..legal
        };
        assert_eq!(steal_action(&view, &facing_bet, true), Action::Fold);
    }

    #[test]
    fn steal_falls_back_without_calling_when_raise_is_unavailable() {
        let view = HandView {
            street: "Preflop".into(),
            button: 0,
            big_blind: 2,
            board: Vec::new(),
            your_hole_cards: None,
            seats: Vec::new(),
            pot: 200,
            current_player: Some(0),
            legal_actions: None,
            summary: None,
            players: Vec::new(),
            events: Vec::new(),
            last_bet: 0,
            to_call: 0,
            awaiting_advance: false,
            runout_leaders: Vec::new(),
            runout_odds: Vec::new(),
        };
        let free_only = LegalActions {
            seat: 0,
            actions: vec![Action::Fold, Action::Check],
            to_call: 0,
            wager: None,
            wagers_capped: true,
        };
        assert_eq!(
            steal_action(&view, &free_only, false),
            Action::Check,
            "an unraised hand should take the free fallback"
        );

        let facing_call = LegalActions {
            actions: vec![Action::Fold, Action::Call],
            to_call: 100,
            ..free_only
        };
        let called_view = HandView {
            events: vec![two_seven::holdem::HandEvent {
                street: Street::Preflop,
                seat: Some(1),
                kind: HandEventKind::Call,
                amount: 100,
            }],
            ..view
        };
        assert_eq!(
            steal_action(&called_view, &facing_call, false),
            Action::Fold,
            "a facing call must never become a call fallback"
        );
    }

    #[test]
    fn repeated_bench_bots_get_distinct_output_labels() {
        let lineup = vec![
            parse_kind("shark:default"),
            parse_kind("shark:default"),
            parse_kind("steal"),
        ];
        assert_eq!(
            display_labels(&lineup),
            vec!["shark:default (seat 0)", "shark:default (seat 1)", "steal",]
        );
    }

    #[test]
    fn bench_stakes_preserve_the_existing_default_and_limit_convention() {
        assert_eq!(
            parse_stakes("no-limit"),
            Stakes::NoLimit {
                small_blind: 100,
                big_blind: 200,
            }
        );
        assert_eq!(
            parse_stakes("limit"),
            Stakes::Limit {
                small_bet: 200,
                big_bet: 400,
            }
        );
        assert_eq!(parse_stakes("limit").blinds(), (100, 200));
    }

    #[test]
    fn shark_threshold_presets_match_their_names() {
        let default = shark_preset("default").unwrap();
        let conservative = shark_preset("conservative").unwrap();
        let phase1 = shark_preset("phase1").unwrap();
        assert_eq!(default.late_open_score, 3);
        assert_eq!(conservative.late_open_score, 4);
        assert_eq!(phase1.late_open_score, conservative.late_open_score);
        assert_eq!(phase1.heads_up_in_position_edge, 0.10);
        assert_eq!(default.heads_up_in_position_edge, 0.08);
    }

    #[test]
    fn shark_registered_presets_have_unique_parameters() {
        let names = [
            "default",
            "phase1",
            "conservative",
            "nit",
            "aggro",
            "features",
            "aggro_noprobe",
            "tuned",
            "samples25",
            "samples50",
            "samples200",
            "samples400",
            "samples64",
        ];
        let presets: Vec<_> = names
            .iter()
            .map(|name| (*name, shark_preset(name).unwrap()))
            .collect();
        for (index, (name, params)) in presets.iter().enumerate() {
            for (other_name, other_params) in presets.iter().skip(index + 1) {
                assert_ne!(
                    params, other_params,
                    "presets {name} and {other_name} unexpectedly match"
                );
            }
        }
    }

    #[test]
    fn shark_sampling_presets_only_change_sample_counts() {
        for (name, flop, turn, river) in [
            ("samples25", 40, 56, 80),
            ("samples50", 80, 112, 160),
            ("samples200", 320, 448, 640),
            ("samples400", 640, 896, 1280),
            ("samples64", 64, 64, 64),
        ] {
            let mut expected = SharkParams::DEFAULT;
            expected.flop_samples = flop;
            expected.turn_samples = turn;
            expected.river_samples = river;
            assert_eq!(shark_preset(name), Some(expected));
        }
    }

    #[test]
    #[should_panic(expected = "unknown Shark preset: unknown")]
    fn unknown_shark_preset_is_rejected() {
        parse_kind("shark:unknown");
    }
}
