use super::{first, first_calling};
use crate::{
    cards::{Card, Deck, Rank},
    eval::evaluate,
    holdem::{Action, LegalActions},
    money::Cents,
    view::HandView,
};
use rand::{Rng, SeedableRng, rngs::StdRng};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SharkRatio {
    pub numerator: u64,
    pub denominator: u64,
}

impl SharkRatio {
    pub const fn new(numerator: u64, denominator: u64) -> Self {
        Self {
            numerator,
            denominator,
        }
    }

    fn apply_cents(self, amount: Cents) -> Cents {
        if self.denominator == 0 {
            return amount;
        }
        amount.saturating_mul(self.numerator as Cents) / self.denominator as Cents
    }

    fn apply_count(self, amount: usize) -> usize {
        if self.denominator == 0 {
            return amount;
        }
        amount.saturating_mul(self.numerator as usize) / self.denominator as usize
    }

    fn reaches(self, amount: Cents, total: Cents) -> bool {
        self.denominator == 0
            || amount.saturating_mul(self.denominator as Cents)
                >= total.saturating_mul(self.numerator as Cents)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SharkFrequency {
    pub numerator: u32,
    pub denominator: u32,
}

impl SharkFrequency {
    fn succeeds(self, rng: &mut StdRng) -> bool {
        rng.gen_ratio(self.numerator, self.denominator)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SharkParams {
    /// Chen score required to open on the button.
    pub late_open_score: i32,
    /// Chen score required to open from the cutoff and nearby seats.
    pub middle_open_score: i32,
    /// Chen score required to open from early position.
    pub early_open_score: i32,
    /// Maximum live seats behind hero for the late-position threshold.
    pub late_open_max_behind: usize,
    /// Maximum live seats behind hero for the middle-position threshold.
    pub middle_open_max_behind: usize,
    /// Minimum Chen score for a big-blind defense.
    pub big_blind_defense_score: i32,
    /// Minimum Chen score for a small-blind defense.
    pub small_blind_defense_score: i32,
    /// Minimum Chen score for defense outside the blinds.
    pub other_defense_score: i32,
    /// Largest pot-odds price accepted by the big blind.
    pub big_blind_defense_pot_odds: f64,
    /// Largest pot-odds price accepted by the small blind.
    pub small_blind_defense_pot_odds: f64,
    /// Chen score treated as a premium preflop holding.
    pub premium_score: i32,
    /// Pot fraction used for a premium raise.
    pub premium_raise_ratio: SharkRatio,
    /// Pot fraction used for a standard open or three-bet.
    pub open_raise_ratio: SharkRatio,
    /// Chen score that permits a cheap preflop three-bet.
    pub three_bet_score: i32,
    /// Maximum pot-odds price for the cheap three-bet line.
    pub three_bet_pot_odds: f64,
    /// Fraction of marginal opens that raise rather than limp or call.
    pub marginal_open_raise_frequency: SharkFrequency,
    /// Effective stack depth below which preflop play becomes shove-or-fold.
    pub short_stack_bb: Cents,
    /// Minimum equity edge for a heads-up bet in position.
    pub heads_up_in_position_edge: f64,
    /// Minimum equity edge for a heads-up bet out of position.
    pub heads_up_out_of_position_edge: f64,
    /// Minimum equity edge for a multiway bet in position.
    pub multiway_in_position_edge: f64,
    /// Minimum equity edge for a multiway bet out of position.
    pub multiway_out_of_position_edge: f64,
    /// Maximum live seats behind hero to count as in position.
    pub in_position_max_behind: usize,
    /// Absolute equity at which value betting overrides the required edge.
    pub absolute_value_equity: f64,
    /// Minimum edge for an in-position semi-bluff.
    pub semi_bluff_edge: f64,
    /// Frequency of the in-position semi-bluff.
    pub semi_bluff_frequency: SharkFrequency,
    /// Pot fraction used for a value bet.
    pub value_bet_ratio: SharkRatio,
    /// Pot fraction used for a semi-bluff.
    pub semi_bluff_ratio: SharkRatio,
    /// Effective-stack fraction at which a wager commits the stack.
    pub all_in_threshold: SharkRatio,
    /// Monte Carlo samples on the flop.
    pub flop_samples: usize,
    /// Monte Carlo samples on the turn.
    pub turn_samples: usize,
    /// Monte Carlo samples on the river.
    pub river_samples: usize,
    /// Equity used when the hero's hole cards are unavailable or malformed.
    pub missing_cards_equity: f64,
    /// Sample fraction retained against two opponents.
    pub two_opponent_sample_ratio: SharkRatio,
    /// Sample fraction retained against three or more opponents.
    pub multiway_sample_ratio: SharkRatio,
    /// Chen score required for an aggressive opponent range.
    pub aggressive_range_score: i32,
    /// Chen score required for a caller's opponent range.
    pub caller_range_score: i32,
    /// Maximum rejected hole-card draws for an observed range.
    pub range_rejection_attempts: usize,
}

impl SharkParams {
    pub const DEFAULT: Self = Self {
        late_open_score: 4,
        middle_open_score: 5,
        early_open_score: 6,
        late_open_max_behind: 0,
        middle_open_max_behind: 2,
        big_blind_defense_score: 3,
        small_blind_defense_score: 4,
        other_defense_score: 6,
        big_blind_defense_pot_odds: 0.32,
        small_blind_defense_pot_odds: 0.25,
        premium_score: 10,
        premium_raise_ratio: SharkRatio::new(3, 2),
        open_raise_ratio: SharkRatio::new(1, 1),
        three_bet_score: 9,
        three_bet_pot_odds: 0.30,
        marginal_open_raise_frequency: SharkFrequency {
            numerator: 4,
            denominator: 5,
        },
        short_stack_bb: 20,
        heads_up_in_position_edge: 0.10,
        heads_up_out_of_position_edge: 0.16,
        multiway_in_position_edge: 0.14,
        multiway_out_of_position_edge: 0.20,
        in_position_max_behind: 1,
        absolute_value_equity: 0.72,
        semi_bluff_edge: 0.05,
        semi_bluff_frequency: SharkFrequency {
            numerator: 1,
            denominator: 4,
        },
        value_bet_ratio: SharkRatio::new(2, 3),
        semi_bluff_ratio: SharkRatio::new(1, 2),
        all_in_threshold: SharkRatio::new(2, 3),
        flop_samples: 160,
        turn_samples: 224,
        river_samples: 320,
        missing_cards_equity: 0.25,
        two_opponent_sample_ratio: SharkRatio::new(3, 4),
        multiway_sample_ratio: SharkRatio::new(1, 2),
        aggressive_range_score: 7,
        caller_range_score: 4,
        range_rejection_attempts: 12,
    };
}

pub fn shark_with(
    params: &SharkParams,
    view: &HandView,
    legal: &LegalActions,
    seed: u64,
) -> Action {
    if view.board.is_empty() {
        return shark_preflop(params, view, legal, seed);
    }
    let mut rng = StdRng::seed_from_u64(seed ^ 0x0053_4841_524b);
    let opponents = active_opponents(view, legal.seat);
    let behind = players_behind(view, legal.seat);
    let in_position = behind <= params.in_position_max_behind;
    let effective = effective_stack(view, legal.seat);
    let equity = estimate_equity(params, view, legal.seat, seed, opponents);
    let fair_share = 1.0 / (opponents + 1) as f64;
    let pot_odds = if legal.to_call == 0 {
        0.0
    } else {
        legal.to_call as f64 / (view.pot + legal.to_call) as f64
    };
    let edge = equity - fair_share;
    let bet_edge = if opponents == 1 {
        if in_position {
            params.heads_up_in_position_edge
        } else {
            params.heads_up_out_of_position_edge
        }
    } else if in_position {
        params.multiway_in_position_edge
    } else {
        params.multiway_out_of_position_edge
    };
    if edge >= bet_edge || equity > params.absolute_value_equity {
        let target = legal.to_call + params.value_bet_ratio.apply_cents(view.pot);
        return sized_wager(params, legal, target, effective)
            .unwrap_or_else(|| first_calling(legal));
    }
    if in_position
        && edge > params.semi_bluff_edge
        && params.semi_bluff_frequency.succeeds(&mut rng)
    {
        let target = legal.to_call + params.semi_bluff_ratio.apply_cents(view.pot);
        return sized_wager(params, legal, target, effective)
            .unwrap_or_else(|| first_calling(legal));
    }
    if legal.to_call > 0 && equity < pot_odds {
        return first(legal, Action::Fold);
    }
    first_calling(legal)
}

#[cfg(test)]
pub(super) fn shark(view: &HandView, legal: &LegalActions, seed: u64) -> Action {
    shark_with(&SharkParams::DEFAULT, view, legal, seed)
}

fn shark_preflop(params: &SharkParams, view: &HandView, legal: &LegalActions, seed: u64) -> Action {
    let mut rng = StdRng::seed_from_u64(seed ^ 0x0050_4645);
    let score = preflop_score(view);
    let (small_blind, big_blind) = blind_seats(view);
    let is_big_blind = big_blind == Some(legal.seat);
    let is_small_blind = small_blind == Some(legal.seat);
    let unraised = !view.events.iter().any(|event| {
        event.street == crate::holdem::Street::Preflop
            && matches!(
                event.kind,
                crate::holdem::HandEventKind::Bet
                    | crate::holdem::HandEventKind::Raise
                    | crate::holdem::HandEventKind::AllIn
            )
    });
    let pot_odds = if legal.to_call == 0 {
        0.0
    } else {
        legal.to_call as f64 / (view.pot + legal.to_call) as f64
    };
    let open_threshold = opening_threshold(params, view, legal.seat);
    let defense_threshold = if is_big_blind {
        params.big_blind_defense_score
    } else if is_small_blind {
        params.small_blind_defense_score
    } else {
        params.other_defense_score
    };
    let effective = effective_stack(view, legal.seat);
    let short = effective < view.big_blind * params.short_stack_bb;
    let single_raise = view
        .players
        .iter()
        .filter(|player| player.seat != legal.seat && !player.folded)
        .filter(|player| {
            view.events.iter().any(|event| {
                event.street == crate::holdem::Street::Preflop
                    && event.seat == Some(player.seat)
                    && matches!(
                        event.kind,
                        crate::holdem::HandEventKind::Bet
                            | crate::holdem::HandEventKind::Raise
                            | crate::holdem::HandEventKind::AllIn
                    )
            })
        })
        .count()
        == 1;
    if short && score >= open_threshold && (unraised || score >= defense_threshold) {
        return all_in_or_wager(params, legal, effective).unwrap_or_else(|| first_calling(legal));
    }
    if score >= params.premium_score {
        let target = legal.to_call + params.premium_raise_ratio.apply_cents(view.pot);
        return sized_wager(params, legal, target, effective)
            .unwrap_or_else(|| first_calling(legal));
    }
    if unraised && score >= open_threshold {
        if score == open_threshold && !params.marginal_open_raise_frequency.succeeds(&mut rng) {
            return first_calling(legal);
        }
        let target = legal.to_call + params.open_raise_ratio.apply_cents(view.pot);
        return sized_wager(params, legal, target, effective)
            .unwrap_or_else(|| first_calling(legal));
    }
    if single_raise
        && (is_big_blind || is_small_blind)
        && score >= defense_threshold
        && pot_odds
            <= if is_big_blind {
                params.big_blind_defense_pot_odds
            } else {
                params.small_blind_defense_pot_odds
            }
    {
        return first_calling(legal);
    }
    if score >= params.three_bet_score && pot_odds < params.three_bet_pot_odds {
        let target = legal.to_call + params.open_raise_ratio.apply_cents(view.pot);
        return sized_wager(params, legal, target, effective)
            .unwrap_or_else(|| first_calling(legal));
    }
    if legal.to_call == 0 {
        return first_calling(legal);
    }
    first(legal, Action::Fold)
}

/// Rough Chen-style preflop hand score: pairs and big suited/connected
/// combinations score high, offsuit trash scores near zero.
fn preflop_score(view: &HandView) -> i32 {
    let Some(cards) = view.your_hole_cards.as_ref() else {
        return 0;
    };
    preflop_score_cards(cards)
}

fn preflop_score_cards(cards: &[Card]) -> i32 {
    if cards.len() != 2 {
        return 0;
    }
    let (high, low) = if cards[0].rank >= cards[1].rank {
        (cards[0], cards[1])
    } else {
        (cards[1], cards[0])
    };
    let rank_points = |rank: Rank| match rank {
        Rank::Ace => 6,
        Rank::King => 5,
        Rank::Queen => 4,
        Rank::Jack => 3,
        Rank::Ten => 2,
        Rank::Nine | Rank::Eight => 1,
        _ => 0,
    };
    let mut score = rank_points(high.rank) + rank_points(low.rank);
    if high.rank == low.rank {
        score = (score * 2).max(5);
    }
    if high.suit == low.suit {
        score += 1;
    }
    let gap = (high.rank as i32) - (low.rank as i32);
    if gap == 1 {
        score += 1;
    } else if gap > 2 {
        score -= (gap - 2).min(3);
    }
    score
}

fn live_order(view: &HandView) -> Vec<usize> {
    table_order(view)
        .into_iter()
        .filter(|seat| {
            view.players
                .iter()
                .find(|player| player.seat == *seat)
                .is_some_and(|player| !player.folded)
        })
        .collect()
}

fn table_order(view: &HandView) -> Vec<usize> {
    let seats: Vec<usize> = view.players.iter().map(|player| player.seat).collect();
    if seats.is_empty() {
        return seats;
    }
    let start = seats
        .iter()
        .position(|seat| *seat == view.button)
        .unwrap_or(0);
    (0..seats.len())
        .map(|offset| seats[(start + offset) % seats.len()])
        .collect()
}

pub(super) fn players_behind(view: &HandView, seat: usize) -> usize {
    players_behind_excluding(view, seat, [None, None])
}

fn players_behind_excluding(view: &HandView, seat: usize, excluded: [Option<usize>; 2]) -> usize {
    let order = street_action_order(view);
    let Some(hero) = order.iter().position(|candidate| *candidate == seat) else {
        return 0;
    };
    order
        .iter()
        .skip(hero + 1)
        .filter(|candidate| {
            !excluded.contains(&Some(**candidate))
                && view.players.iter().any(|player| {
                    player.seat == **candidate
                        && !player.folded
                        && !player.all_in
                        && (!player.acted || player.street_contribution < view.last_bet)
                })
        })
        .count()
}

fn street_action_order(view: &HandView) -> Vec<usize> {
    let seats = table_order(view);
    if seats.is_empty() {
        return seats;
    }
    let start = if view.board.is_empty() {
        if seats.len() == 2 {
            seats.iter().position(|seat| *seat == view.button)
        } else {
            blind_seats(view)
                .1
                .and_then(|big_blind| seats.iter().position(|seat| *seat == big_blind))
                .map(|index| (index + 1) % seats.len())
        }
    } else {
        seats
            .iter()
            .position(|seat| *seat == view.button)
            .map(|index| (index + 1) % seats.len())
    }
    .unwrap_or(0);
    (0..seats.len())
        .map(|offset| seats[(start + offset) % seats.len()])
        .filter(|seat| {
            view.players
                .iter()
                .find(|player| player.seat == *seat)
                .is_some_and(|player| !player.folded)
        })
        .collect()
}

pub(super) fn opening_threshold(params: &SharkParams, view: &HandView, seat: usize) -> i32 {
    let (small_blind, big_blind) = blind_seats(view);
    let behind = players_behind_excluding(view, seat, [small_blind, big_blind]);
    if behind <= params.late_open_max_behind {
        params.late_open_score
    } else if behind <= params.middle_open_max_behind {
        params.middle_open_score
    } else {
        params.early_open_score
    }
}

fn blind_seats(view: &HandView) -> (Option<usize>, Option<usize>) {
    let small_blind = view.events.iter().find_map(|event| {
        (event.street == crate::holdem::Street::Preflop
            && matches!(event.kind, crate::holdem::HandEventKind::SmallBlind))
        .then_some(event.seat)
        .flatten()
    });
    let big_blind = view.events.iter().find_map(|event| {
        (event.street == crate::holdem::Street::Preflop
            && matches!(event.kind, crate::holdem::HandEventKind::BigBlind))
        .then_some(event.seat)
        .flatten()
    });
    if small_blind.is_some() || big_blind.is_some() {
        return (small_blind, big_blind);
    }
    let order = live_order(view);
    if order.len() < 2 {
        return (None, None);
    }
    if order.len() == 2 {
        (order.first().copied(), order.get(1).copied())
    } else {
        (order.get(1).copied(), order.get(2).copied())
    }
}

fn active_opponents(view: &HandView, seat: usize) -> usize {
    let live = view
        .players
        .iter()
        .filter(|player| player.seat != seat && !player.folded)
        .count();
    live.max(1)
}

fn effective_stack(view: &HandView, seat: usize) -> Cents {
    let hero = view
        .players
        .iter()
        .find(|player| player.seat == seat)
        .map_or(0, |player| player.stack);
    let largest_opponent = view
        .players
        .iter()
        .filter(|player| player.seat != seat && !player.folded)
        .map(|player| player.stack)
        .max()
        .unwrap_or(hero);
    hero.min(largest_opponent)
}

fn all_in_or_wager(params: &SharkParams, legal: &LegalActions, effective: Cents) -> Option<Action> {
    if legal
        .actions
        .iter()
        .any(|action| matches!(action, Action::AllIn))
    {
        Some(Action::AllIn)
    } else {
        sized_wager(params, legal, effective, effective)
    }
}

fn sized_wager(
    params: &SharkParams,
    legal: &LegalActions,
    target: Cents,
    effective: Cents,
) -> Option<Action> {
    if target > 0
        && params.all_in_threshold.reaches(target, effective)
        && legal
            .actions
            .iter()
            .any(|action| matches!(action, Action::AllIn))
    {
        return Some(Action::AllIn);
    }
    legal.actions.iter().find_map(|action| match action {
        Action::Bet { amount } => Some(Action::Bet {
            amount: sized_amount(legal, target, *amount),
        }),
        Action::Raise { amount } => Some(Action::Raise {
            amount: sized_amount(legal, target, *amount),
        }),
        _ => None,
    })
}

fn sized_amount(legal: &LegalActions, target: Cents, offered: Cents) -> Cents {
    match legal.wager {
        Some(bounds) => target.clamp(bounds.min, bounds.max),
        None => offered,
    }
}

fn estimate_equity(
    params: &SharkParams,
    view: &HandView,
    hero_seat: usize,
    seed: u64,
    opponents: usize,
) -> f64 {
    let Some(hero) = view.your_hole_cards.as_ref() else {
        return params.missing_cards_equity;
    };
    if hero.len() != 2 {
        return params.missing_cards_equity;
    }
    let mut deck = Deck::seeded(0);
    let mut unseen: Vec<Card> = (0..52).filter_map(|_| deck.deal()).collect();
    unseen.retain(|card| !hero.contains(card) && !view.board.contains(card));
    let opponents = opponents.max(1);
    let opponent_seats: Vec<usize> = view
        .players
        .iter()
        .filter(|player| player.seat != hero_seat && !player.folded)
        .map(|player| player.seat)
        .collect();
    let mut tiers: Vec<OpponentTier> = opponent_seats
        .iter()
        .map(|seat| opponent_tier(view, *seat))
        .collect();
    if tiers.is_empty() {
        tiers.push(OpponentTier::Passive);
    }
    debug_assert_eq!(tiers.len(), opponents);
    let mut wins = 0.0;
    let street_samples = match view.board.len() {
        3 => params.flop_samples,
        4 => params.turn_samples,
        _ => params.river_samples,
    };
    let samples = match opponents {
        1 => street_samples,
        2 => params.two_opponent_sample_ratio.apply_count(street_samples),
        _ => params.multiway_sample_ratio.apply_count(street_samples),
    };
    let mut rng = StdRng::seed_from_u64(seed ^ 0x0053_4841_524b);
    'sample: for _ in 0..samples {
        let mut available = unseen.clone();
        let mut opponent_hands = Vec::with_capacity(opponents);
        for index in 0..opponents {
            let Some(hand) = sample_opponent(
                params,
                &mut available,
                &mut rng,
                tiers.get(index).copied().unwrap_or(OpponentTier::Passive),
            ) else {
                continue 'sample;
            };
            opponent_hands.push(hand);
        }
        let mut board = view.board.clone();
        if available.len() < 5 - board.len() {
            continue;
        }
        while board.len() < 5 {
            let index = rng.gen_range(0..available.len());
            board.push(available.swap_remove(index));
        }
        let mut hero_cards = hero.clone();
        hero_cards.extend(board.iter().copied());
        let hero_rank = evaluate(&hero_cards).rank;
        let best_opponent = opponent_hands
            .iter()
            .map(|hand| {
                let mut cards = hand.clone();
                cards.extend(board.iter().copied());
                evaluate(&cards).rank
            })
            .max()
            .expect("at least one opponent");
        if hero_rank > best_opponent {
            wins += 1.0;
        } else if hero_rank == best_opponent {
            wins += 0.5;
        }
    }
    wins / samples as f64
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum OpponentTier {
    Aggressive,
    Caller,
    Passive,
}

pub(super) fn opponent_tier(view: &HandView, seat: usize) -> OpponentTier {
    let mut aggressive = false;
    let mut caller = false;
    let mut blind = false;
    for event in view.events.iter().filter(|event| event.seat == Some(seat)) {
        match event.kind {
            crate::holdem::HandEventKind::SmallBlind | crate::holdem::HandEventKind::BigBlind => {
                blind = true
            }
            crate::holdem::HandEventKind::Bet
            | crate::holdem::HandEventKind::Raise
            | crate::holdem::HandEventKind::AllIn => aggressive = true,
            crate::holdem::HandEventKind::Call => caller = true,
            _ => {}
        }
    }
    if aggressive {
        OpponentTier::Aggressive
    } else if caller && !blind {
        OpponentTier::Caller
    } else {
        OpponentTier::Passive
    }
}

pub(super) fn range_accepts(params: &SharkParams, tier: OpponentTier, cards: &[Card]) -> bool {
    match tier {
        OpponentTier::Aggressive => preflop_score_cards(cards) >= params.aggressive_range_score,
        OpponentTier::Caller => preflop_score_cards(cards) >= params.caller_range_score,
        OpponentTier::Passive => true,
    }
}

fn sample_opponent(
    params: &SharkParams,
    available: &mut Vec<Card>,
    rng: &mut StdRng,
    tier: OpponentTier,
) -> Option<Vec<Card>> {
    if available.len() < 2 {
        return None;
    }
    let filtered = !matches!(tier, OpponentTier::Passive);
    for _ in 0..params.range_rejection_attempts {
        let (first, second) = pair_indices(available.len(), rng)?;
        let cards = vec![available[first], available[second]];
        if !filtered || range_accepts(params, tier, &cards) {
            return Some(take_pair(available, first, second));
        }
    }
    pair_indices(available.len(), rng).map(|(first, second)| take_pair(available, first, second))
}

pub(super) fn pair_indices(len: usize, rng: &mut StdRng) -> Option<(usize, usize)> {
    if len < 2 {
        return None;
    }
    let first = rng.gen_range(0..len);
    let offset = rng.gen_range(0..len - 1);
    let second = if offset >= first { offset + 1 } else { offset };
    Some((first, second))
}

fn take_pair(available: &mut Vec<Card>, first: usize, second: usize) -> Vec<Card> {
    let first_card = available.swap_remove(first);
    let second_card = available.swap_remove(if second > first { second - 1 } else { second });
    vec![first_card, second_card]
}
