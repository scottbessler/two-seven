use serde::{Deserialize, Serialize};
use std::fmt;

pub type Cents = i64;
pub const MIN_GAME_AMOUNT: Cents = 100;
/// The most that may ride on anything: the top cash table's entry, and the
/// ceiling a blackjack bet or a tournament blind level may climb to.
pub const MAX_GAME_ENTRY: Cents = 100_000_000;

pub fn valid_game_amount(value: Cents) -> bool {
    (MIN_GAME_AMOUNT..=MAX_GAME_ENTRY).contains(&value)
}

pub fn valid_optional_game_amount(value: Cents) -> bool {
    value == 0 || valid_game_amount(value)
}

pub fn format_cents(value: Cents) -> String {
    let sign = if value < 0 { "-" } else { "" };
    let abs = value.unsigned_abs();
    let dollars = abs / 100;
    let cents = abs % 100;
    let digits = dollars.to_string();
    let mut grouped = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(ch);
    }
    format!("{sign}${grouped}.{cents:02}")
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub struct Money(pub Cents);
impl fmt::Display for Money {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&format_cents(self.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn formats() {
        assert_eq!(format_cents(123456), "$1,234.56");
        assert_eq!(format_cents(-5), "-$0.05");
    }

    #[test]
    fn game_amounts_start_at_a_dollar() {
        assert!(!valid_game_amount(99));
        assert!(valid_game_amount(100));
        assert!(valid_game_amount(1_000_000));
        assert!(valid_optional_game_amount(0));
        assert!(!valid_optional_game_amount(50));
    }

    #[test]
    fn the_ceiling_covers_the_whole_cash_ladder() {
        // The top table costs $1,000,000 to sit down at, and the T10,000
        // tournament ladder tops out at a 16,000-chip big blind.
        assert!(valid_game_amount(100_000_000));
        assert!(valid_game_amount(1_600_000));
        assert!(!valid_game_amount(100_000_001));
    }
}
