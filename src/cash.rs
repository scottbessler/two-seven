//! The standing cash tables.
//!
//! Cash games are not created by players any more: eleven tables always exist,
//! one per entry tier, each seating six for no-limit. Blinds and the mix of
//! house players both follow from the entry, so a bigger table is a harder one.

use crate::{
    money::Cents,
    table::{Bot, BotKind, Seat, SeatOccupant, Stakes, Table, TableMode},
};

/// What it costs to sit down, cheapest first.
pub const TIERS: [Cents; 11] = [
    20_000,
    100_000,
    200_000,
    500_000,
    1_000_000,
    2_000_000,
    5_000_000,
    10_000_000,
    20_000_000,
    50_000_000,
    100_000_000,
];

pub const SEATS: usize = 6;

/// From this rung up the house sits no fish: the stakes are past the point
/// where a player who calls at random belongs in the game (§V62).
pub const NO_FISH_FROM: Cents = 10_000_000;

/// From this rung up the house sits nothing but sharks (§V62).
pub const SHARKS_ONLY_FROM: Cents = 50_000_000;

/// Whether a kind may be seated at a game of this size. The rule is the buy-in,
/// not the ladder index, so a tournament at a rung answers the same as the cash
/// table at it.
pub fn kind_allowed(buy_in: Cents, kind: BotKind) -> bool {
    if buy_in >= SHARKS_ONLY_FROM {
        return kind == BotKind::Shark;
    }
    if buy_in >= NO_FISH_FROM {
        return kind != BotKind::Fish;
    }
    true
}

/// The kinds that may sit at a game of this size, hardest last.
pub fn kinds_allowed(buy_in: Cents) -> Vec<BotKind> {
    BotKind::ALL
        .into_iter()
        .filter(|kind| kind_allowed(buy_in, *kind))
        .collect()
}

/// A hundredth of the entry is the big blind, so a full stack is a hundred
/// blinds at every table on the ladder.
pub fn blinds(buy_in: Cents) -> (Cents, Cents) {
    (buy_in / 200, buy_in / 100)
}

pub fn stakes(buy_in: Cents) -> Stakes {
    let (small_blind, big_blind) = blinds(buy_in);
    Stakes::NoLimit {
        small_blind,
        big_blind,
    }
}

pub fn name(buy_in: Cents) -> String {
    let (small_blind, big_blind) = blinds(buy_in);
    format!(
        "{}/{} No-Limit",
        crate::money::format_cents(small_blind),
        crate::money::format_cents(big_blind)
    )
}

/// Who the house sits at this rung, in the words the lobby uses. Read off the
/// same mix that fills the seats, so the label can never drift from the table.
/// A buy-in that is not a rung at all (nothing builds one today) says nothing
/// rather than guessing.
pub fn house_style(buy_in: Cents) -> Option<&'static str> {
    let tier = TIERS.iter().position(|rung| *rung == buy_in)?;
    let [fish, _, _, sharks] = bot_mix(tier);
    Some(if sharks == 100 {
        "sharks only"
    } else if fish >= 50 {
        "mostly fish"
    } else if sharks >= 50 {
        "mostly sharks"
    } else {
        "mixed house"
    })
}

/// How the house fills a table, as percentages in roster order. The cheap
/// tables are mostly fish; the dearest is nothing but sharks.
pub fn bot_mix(tier: usize) -> [u32; 4] {
    // Three bands, each an even slide between its endpoints: soft up to the
    // no-fish rung, fish-free from there to the shark-only rung, and nobody
    // but sharks above it. The sharks take whatever the others give up, which
    // keeps the mix at exactly a hundred.
    const CHEAPEST: [u32; 4] = [60, 20, 10, 10];
    const NO_FISH: [u32; 4] = [0, 40, 30, 30];
    const SHARKS_ONLY: [u32; 4] = [0, 0, 0, 100];
    let buy_in = TIERS[tier.min(TIERS.len() - 1)];
    if buy_in >= SHARKS_ONLY_FROM {
        return SHARKS_ONLY;
    }
    let rungs = |from: Cents, to: Cents| {
        TIERS
            .iter()
            .filter(|entry| **entry >= from && **entry < to)
            .count()
    };
    let (start, end, step, span) = if buy_in >= NO_FISH_FROM {
        // Between the thresholds the fish are already gone and the rest thin
        // out until only sharks are left at the top of the band.
        let step = TIERS[..tier]
            .iter()
            .filter(|entry| **entry >= NO_FISH_FROM)
            .count();
        (
            NO_FISH,
            SHARKS_ONLY,
            step,
            rungs(NO_FISH_FROM, SHARKS_ONLY_FROM),
        )
    } else {
        // Below the threshold the fish thin out over the whole band, so the
        // last soft rung hands over to a table that has none.
        (
            CHEAPEST,
            NO_FISH,
            tier,
            rungs(0, NO_FISH_FROM).saturating_sub(1),
        )
    };
    let span = span.max(1);
    let mut mix = SHARKS_ONLY;
    let mut given = 0;
    for index in 0..3 {
        let from = i64::from(start[index]);
        let to = i64::from(end[index]);
        let moved = (to - from) * step.min(span) as i64 / span as i64;
        mix[index] = (from + moved) as u32;
        given += mix[index];
    }
    mix[3] = 100 - given;
    mix
}

/// The kinds a table draws from, in the order the house seats them: the mix
/// spread across the six seats, so a $200 table gets roughly four fish.
pub fn seating_order(tier: usize) -> Vec<BotKind> {
    let mix = bot_mix(tier);
    let kinds = [
        BotKind::Fish,
        BotKind::Grinder,
        BotKind::Rock,
        BotKind::Shark,
    ];
    // Largest remainder, so six seats reflect the percentages as closely as
    // six seats can.
    let mut counts: Vec<(usize, u32, u32)> = mix
        .iter()
        .enumerate()
        .map(|(index, percent)| {
            let exact = percent * SEATS as u32;
            (index, exact / 100, exact % 100)
        })
        .collect();
    let mut seated: u32 = counts.iter().map(|(_, whole, _)| whole).sum();
    counts.sort_by(|left, right| right.2.cmp(&left.2).then(left.0.cmp(&right.0)));
    for entry in counts.iter_mut() {
        if seated as usize >= SEATS {
            break;
        }
        if entry.2 > 0 || seated == 0 {
            entry.1 += 1;
            seated += 1;
        }
    }
    counts.sort_by_key(|(index, _, _)| *index);
    let mut order = Vec::new();
    for (index, count, _) in counts {
        for _ in 0..count {
            order.push(kinds[index]);
        }
    }
    order.truncate(SEATS);
    order
}

/// A table for a tier, with every seat empty; the house fills them.
pub fn table(tier: usize) -> Table {
    let buy_in = TIERS[tier];
    let mut table = Table::new(
        name(buy_in),
        stakes(buy_in),
        TableMode::Cash { no_debt: false },
        SEATS,
        buy_in,
    );
    table.cash_tier = Some(tier);
    table
}

/// Who the house would put in an empty seat, avoiding anyone already here.
pub fn house_bot(table: &Table, tier: usize, seat: usize) -> Option<Bot> {
    let order = seating_order(tier);
    let kind = *order.get(seat % order.len())?;
    // Prefer the kind this seat calls for, then anyone else who is free.
    let free = |kind: BotKind| {
        (0..kind.regulars())
            .map(move |index| Bot::new(kind, index))
            .find(|bot| {
                !table
                    .seats
                    .iter()
                    .any(|seat| seat.occupant.as_bot() == Some(*bot))
            })
    };
    // The seat's own kind first, then anyone else the rung allows -- never
    // somebody this table is too dear for (§V62).
    free(kind)
        .or_else(|| order.iter().copied().find_map(free))
        .or_else(|| kinds_allowed(TIERS[tier]).into_iter().find_map(free))
}

/// The first empty seat, or a bot's seat when a human needs one and the table
/// is otherwise full. A human never displaces another human.
/// The seat a person sits in: an empty one, or a house player's if there is
/// none. A seat somebody else has already paid to take is spoken for, so it is
/// not on offer even while the house is still sitting in it.
pub fn seat_for_human(seats: &[Seat]) -> Option<usize> {
    seats
        .iter()
        .position(|seat| {
            matches!(seat.occupant, SeatOccupant::Empty) && seat.pending_arrival.is_none()
        })
        .or_else(|| {
            seats
                .iter()
                .position(|seat| seat.occupant.as_bot().is_some() && seat.pending_arrival.is_none())
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blinds_are_a_hundredth_of_the_entry() {
        assert_eq!(blinds(20_000), (100, 200));
        assert_eq!(blinds(100_000), (500, 1_000));
        assert_eq!(blinds(100_000_000), (500_000, 1_000_000));
        // Every table is a hundred big blinds deep.
        for tier in TIERS {
            assert_eq!(tier / blinds(tier).1, 100);
        }
    }

    #[test]
    fn the_mix_slides_from_mostly_fish_to_all_sharks() {
        assert_eq!(bot_mix(0), [60, 20, 10, 10]);
        // The fish thin out over the soft band and are gone by the rung below
        // the no-fish threshold; the last two rungs are sharks alone.
        assert_eq!(bot_mix(1), [50, 23, 13, 14]);
        assert_eq!(bot_mix(6), [0, 40, 30, 30]);
        assert_eq!(bot_mix(TIERS.len() - 1), [0, 0, 0, 100]);
        for (tier, buy_in) in TIERS.into_iter().enumerate() {
            assert_eq!(
                bot_mix(tier)[0] == 0,
                buy_in >= 5_000_000,
                "fish belong below the $50,000 rung and nowhere above it: tier {tier}"
            );
        }
        for tier in 0..TIERS.len() {
            let mix = bot_mix(tier);
            assert_eq!(mix.iter().sum::<u32>(), 100, "tier {tier} must total 100");
            assert!(
                mix[3] >= bot_mix(tier.saturating_sub(1))[3],
                "sharks never thin out as the stakes climb: tier {tier}"
            );
        }
    }

    #[test]
    fn the_house_style_reads_off_the_mix_that_seats_the_table() {
        assert_eq!(house_style(TIERS[0]), Some("mostly fish"));
        assert_eq!(house_style(SHARKS_ONLY_FROM), Some("sharks only"));
        assert_eq!(house_style(*TIERS.last().unwrap()), Some("sharks only"));
        assert_eq!(house_style(1), None);
        // Every rung says something, and no rung claims fish it may not seat.
        for (tier, buy_in) in TIERS.into_iter().enumerate() {
            let style = house_style(buy_in).expect("a rung describes itself");
            assert_eq!(
                style == "sharks only",
                bot_mix(tier)[3] == 100,
                "tier {tier} says {style}"
            );
            assert!(
                style != "mostly fish" || kind_allowed(buy_in, BotKind::Fish),
                "tier {tier} claims fish it may not seat"
            );
        }
    }

    #[test]
    fn the_stakes_decide_who_the_house_will_sit() {
        // Fish are gone from the $100,000 rung up; from $500,000 it is sharks
        // only.
        assert_eq!(kinds_allowed(20_000), BotKind::ALL.to_vec());
        assert_eq!(
            kinds_allowed(NO_FISH_FROM),
            vec![BotKind::Rock, BotKind::Grinder, BotKind::Shark]
        );
        assert_eq!(kinds_allowed(SHARKS_ONLY_FROM), vec![BotKind::Shark]);
        assert_eq!(kinds_allowed(100_000_000), vec![BotKind::Shark]);
        for (tier, buy_in) in TIERS.into_iter().enumerate() {
            let mix = bot_mix(tier);
            for (index, kind) in [
                BotKind::Fish,
                BotKind::Grinder,
                BotKind::Rock,
                BotKind::Shark,
            ]
            .into_iter()
            .enumerate()
            {
                assert!(
                    kind_allowed(buy_in, kind) || mix[index] == 0,
                    "tier {tier} mixes in a {kind} it does not allow"
                );
            }
            for kind in seating_order(tier) {
                assert!(
                    kind_allowed(buy_in, kind),
                    "tier {tier} seats a {kind} it does not allow"
                );
            }
        }
    }

    #[test]
    fn every_table_seats_six_house_players() {
        for tier in 0..TIERS.len() {
            assert_eq!(seating_order(tier).len(), SEATS, "tier {tier}");
        }
        assert!(
            seating_order(TIERS.len() - 1)
                .iter()
                .all(|kind| *kind == BotKind::Shark),
            "the dearest table is nothing but sharks"
        );
        // Nobody is seated at a table their kind is not allowed at (§V62).
        for (tier, buy_in) in TIERS.into_iter().enumerate() {
            let order = seating_order(tier);
            for kind in &order {
                assert!(
                    kind_allowed(buy_in, *kind),
                    "tier {tier} seats a {kind}: {order:?}"
                );
            }
        }
        let cheapest = seating_order(0);
        assert!(
            cheapest
                .iter()
                .filter(|kind| **kind == BotKind::Fish)
                .count()
                >= 3,
            "the cheapest table is mostly fish: {cheapest:?}"
        );
    }

    #[test]
    fn every_tier_can_fill_all_six_seats() {
        // The dearest tables want only sharks, and there are five of them.
        for tier in 0..TIERS.len() {
            let mut table = table(tier);
            for seat in 0..SEATS {
                let bot = house_bot(&table, tier, seat)
                    .unwrap_or_else(|| panic!("tier {tier} seat {seat} found nobody"));
                table.seats[seat].occupant = SeatOccupant::bot(bot);
            }
            assert!(
                table
                    .seats
                    .iter()
                    .all(|seat| seat.occupant.as_bot().is_some()),
                "tier {tier} should fill"
            );
        }
    }

    #[test]
    fn the_house_never_seats_the_same_bot_twice() {
        let mut table = table(0);
        for seat in 0..SEATS {
            let bot = house_bot(&table, 0, seat).expect("a free regular");
            table.seats[seat].occupant = SeatOccupant::bot(bot);
        }
        let seated: std::collections::BTreeSet<_> = table
            .seats
            .iter()
            .filter_map(|seat| seat.occupant.as_bot())
            .collect();
        assert_eq!(seated.len(), SEATS);
    }
}
