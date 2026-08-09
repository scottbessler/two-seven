use serde::{Deserialize, Serialize};
use std::fmt;

pub type Cents = i64;

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
}
