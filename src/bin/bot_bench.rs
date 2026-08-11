//! Bot-vs-bot benchmark: seats bots at a no-limit table, plays out many
//! hands with stacks topped up to 100bb each hand, and prints per-bot stats.
//!
//! Usage: `cargo run --release --bin bot_bench -- [hands] [seed] [lineup]`
//! where `lineup` is a comma-separated list of bot kinds, e.g.
//! `fish,rock,grinder,shark` (the default).

use std::collections::BTreeMap;
use std::str::FromStr;

use two_seven::{
    holdem::{Action, Hand, Street},
    table::{BotKind, Stakes},
    view::hand_view,
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

fn parse_kind(name: &str) -> BotKind {
    match name.trim().to_ascii_lowercase().as_str() {
        "fish" => BotKind::Fish,
        "rock" => BotKind::Rock,
        "grinder" => BotKind::Grinder,
        "shark" => BotKind::Shark,
        other => panic!("unknown bot kind: {other}"),
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
    let lineup: Vec<BotKind> = args
        .get(2)
        .map(|value| value.split(',').map(parse_kind).collect())
        .unwrap_or_else(|| {
            vec![
                BotKind::Fish,
                BotKind::Rock,
                BotKind::Grinder,
                BotKind::Shark,
            ]
        });
    assert!(
        (2..=9).contains(&lineup.len()),
        "lineup must have 2-9 seats"
    );

    let big_blind = 200;
    let stakes = Stakes::NoLimit {
        small_blind: 100,
        big_blind,
    };
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
            let view = hand_view(&hand, Some(seat));
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
        "{hands} hands, {}-handed, blinds {}/{} (100bb stacks, topped up each hand), seed {base_seed}",
        lineup.len(),
        100,
        big_blind
    );
    println!();
    println!(
        "| bot | net | bb/100 | win% | vpip% | pfr% | wtsd% | w$sd% | bet | raise | call | check | fold | AF |"
    );
    println!("|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|");
    for (seat, kind) in lineup.iter().enumerate() {
        let s = &stats[seat];
        let hands_f = s.hands as f64;
        let pct = |count: u64| 100.0 * count as f64 / hands_f;
        let aggression = (s.bets + s.raises + s.all_ins) as f64 / (s.calls.max(1)) as f64;
        println!(
            "| {} | {} | {:.1} | {:.1}% | {:.1}% | {:.1}% | {:.1}% | {:.1}% | {} | {} | {} | {} | {} | {:.2} |",
            kind,
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
