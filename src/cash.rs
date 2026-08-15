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

/// How the house fills a table, as percentages in roster order. The cheap
/// tables are mostly fish; the dearest is nothing but sharks.
pub fn bot_mix(tier: usize) -> [u32; 4] {
    const CHEAPEST: [u32; 4] = [60, 20, 10, 10];
    const SECOND: [u32; 4] = [30, 30, 20, 20];
    const DEAREST: [u32; 4] = [0, 0, 0, 100];
    let last = TIERS.len() - 1;
    match tier {
        0 => CHEAPEST,
        _ => {
            // Everything above the second tier thins the softer players out at
            // an even rate; the sharks take whatever the others give up, which
            // keeps the mix at exactly a hundred.
            let span = (last - 1) as u32;
            let step = (tier.min(last) - 1) as u32;
            let mut mix = DEAREST;
            let mut given = 0;
            for index in 0..3 {
                mix[index] = SECOND[index] - (SECOND[index] * step) / span;
                given += mix[index];
            }
            mix[3] = 100 - given;
            mix
        }
    }
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
        (0..Bot::PER_KIND)
            .map(move |index| Bot::new(kind, index))
            .find(|bot| {
                !table
                    .seats
                    .iter()
                    .any(|seat| seat.occupant.as_bot() == Some(*bot))
            })
    };
    free(kind).or_else(|| order.iter().copied().find_map(free))
}

/// The first empty seat, or a bot's seat when a human needs one and the table
/// is otherwise full. A human never displaces another human.
pub fn seat_for_human(seats: &[Seat]) -> Option<usize> {
    seats
        .iter()
        .position(|seat| matches!(seat.occupant, SeatOccupant::Empty))
        .or_else(|| {
            seats
                .iter()
                .position(|seat| seat.occupant.as_bot().is_some())
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
        assert_eq!(bot_mix(1), [30, 30, 20, 20]);
        assert_eq!(bot_mix(TIERS.len() - 1), [0, 0, 0, 100]);
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
