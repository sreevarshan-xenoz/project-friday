//! Numbers as words (a port of the first half of dizione's `normalize.py`).

const ONES: [&str; 20] = [
    "zero",
    "one",
    "two",
    "three",
    "four",
    "five",
    "six",
    "seven",
    "eight",
    "nine",
    "ten",
    "eleven",
    "twelve",
    "thirteen",
    "fourteen",
    "fifteen",
    "sixteen",
    "seventeen",
    "eighteen",
    "nineteen",
];
const TENS: [&str; 8] = [
    "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety",
];
const SCALES: [&str; 6] = [
    "thousand",
    "million",
    "billion",
    "trillion",
    "quadrillion",
    "quintillion",
];

/// `int(text)` for the ASCII digits of `text` (anything else is ignored; too long saturates).
pub fn int(text: &str) -> i128 {
    text.chars()
        .filter_map(|c| c.to_digit(10))
        .fold(0i128, |n, d| n.saturating_mul(10).saturating_add(d as i128))
}

/// `1234` -> "one thousand two hundred thirty four".
pub fn cardinal(number: i128) -> String {
    if number < 0 {
        return format!("minus {}", cardinal(-number));
    }
    if number < 20 {
        return ONES[number as usize].to_string();
    }
    if number < 100 {
        let (tens, ones) = (number / 10, number % 10);
        return TENS[tens as usize - 2].to_string()
            + &if ones > 0 {
                format!(" {}", ONES[ones as usize])
            } else {
                String::new()
            };
    }
    if number < 1000 {
        let (hundreds, rest) = (number / 100, number % 100);
        return format!("{} hundred", ONES[hundreds as usize])
            + &if rest > 0 {
                format!(" {}", cardinal(rest))
            } else {
                String::new()
            };
    }
    if number >= 1000i128.saturating_pow(SCALES.len() as u32 + 1) {
        return digits(&number.to_string(), "zero");
    }
    let mut parts = Vec::new();
    let mut number = number;
    for power in (1..=SCALES.len()).rev() {
        let unit = 1000i128.pow(power as u32);
        let (count, rest) = (number / unit, number % unit);
        number = rest;
        if count > 0 {
            parts.push(format!("{} {}", cardinal(count), SCALES[power - 1]));
        }
    }
    if number > 0 {
        parts.push(cardinal(number));
    }
    parts.join(" ")
}

pub fn ordinal(number: i128) -> String {
    let text = cardinal(number);
    let mut words: Vec<String> = text.split(' ').map(String::from).collect();
    let last = words.pop().unwrap_or_default();
    let new = match last.as_str() {
        "one" => "first".to_string(),
        "two" => "second".to_string(),
        "three" => "third".to_string(),
        "five" => "fifth".to_string(),
        "eight" => "eighth".to_string(),
        "nine" => "ninth".to_string(),
        "twelve" => "twelfth".to_string(),
        l if l.ends_with('y') => format!("{}ieth", &l[..l.len() - 1]),
        l => format!("{l}th"),
    };
    words.push(new);
    words.join(" ")
}

pub fn digits(text: &str, zero: &str) -> String {
    text.chars()
        .filter_map(|c| c.to_digit(10))
        .map(|d| {
            if d == 0 {
                zero.to_string()
            } else {
                ONES[d as usize].to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The way a year is said: 1984, 1905, 1900, 2005, 2026.
pub fn year(number: i128) -> String {
    if !(1000..=2999).contains(&number) {
        return cardinal(number);
    }
    let (high, low) = (number / 100, number % 100);
    if low == 0 {
        return if high % 10 == 0 {
            cardinal(number)
        } else {
            format!("{} hundred", cardinal(high))
        };
    }
    if high == 20 && low < 10 {
        return format!("two thousand {}", ONES[low as usize]);
    }
    if low < 10 {
        format!("{} oh {}", cardinal(high), ONES[low as usize])
    } else {
        format!("{} {}", cardinal(high), cardinal(low))
    }
}

/// A number read two digits at a time: 4070 "forty seventy", 9001 "ninety oh one".
pub fn pairs(number: i128) -> String {
    let (high, low) = (number / 100, number % 100);
    if !(100..=9999).contains(&number) || (low == 0 && (high < 10 || high % 10 == 0)) {
        return cardinal(number);
    }
    if low == 0 {
        return format!("{} hundred", cardinal(high));
    }
    if low < 10 {
        format!("{} oh {}", cardinal(high), ONES[low as usize])
    } else {
        format!("{} {}", cardinal(high), cardinal(low))
    }
}

/// An unsigned number as written: `1,234`, `3.14`, `007`.
pub fn number_words(text: &str) -> String {
    let cleaned = text.replace(',', "");
    let (whole, fraction) = match cleaned.split_once('.') {
        Some((w, f)) => (w.to_string(), f.to_string()),
        None => (cleaned.clone(), String::new()),
    };
    if !fraction.is_empty() {
        let head = if whole.is_empty() {
            String::new()
        } else {
            cardinal(int(&whole))
        };
        return format!("{head} point {}", digits(&fraction, "zero"));
    }
    if (whole.chars().count() > 1 && whole.starts_with('0')) || whole.chars().count() > 18 {
        return digits(&whole, "zero");
    }
    cardinal(int(&whole))
}

/// `2:03:45` as a length of time.
pub fn duration(hours: i128, minutes: i128, seconds: i128) -> String {
    let parts: Vec<String> = [(hours, "hour"), (minutes, "minute"), (seconds, "second")]
        .iter()
        .filter(|(count, _)| *count != 0)
        .map(|(count, name)| {
            format!(
                "{} {}",
                cardinal(*count),
                if *count == 1 {
                    name.to_string()
                } else {
                    format!("{name}s")
                }
            )
        })
        .collect();
    match parts.len() {
        0 => "zero seconds".to_string(),
        1 => parts[0].clone(),
        n => format!("{} and {}", parts[..n - 1].join(", "), parts[n - 1]),
    }
}

/// Plural of a spelled number, for decades: "nineteen nineties".
pub fn plural_of(words: &str) -> String {
    let (head, last) = match words.rsplit_once(' ') {
        Some((h, l)) => (format!("{h} "), l),
        None => (String::new(), words),
    };
    let last = if let Some(stem) = last.strip_suffix('y') {
        format!("{stem}ies")
    } else {
        format!("{last}s")
    };
    format!("{head}{last}")
}

fn fraction_names(bottom: i128) -> (String, String) {
    match bottom {
        2 => ("half".into(), "halves".into()),
        4 => ("quarter".into(), "quarters".into()),
        _ => (ordinal(bottom), format!("{}s", ordinal(bottom))),
    }
}

/// "three quarters", "one half"; after a whole number "a half", "an eighth".
pub fn fraction_words(top: i128, bottom: i128, article: bool) -> String {
    let (one, many) = fraction_names(bottom);
    if top != 1 {
        return format!("{} {many}", cardinal(top));
    }
    let lead = if article {
        if one.starts_with(['a', 'e', 'i', 'o', 'u']) {
            "an "
        } else {
            "a "
        }
    } else {
        "one "
    };
    format!("{lead}{one}")
}

/// One part of a version number: "4" four, "12" twelve, "03" zero three.
pub fn version_part(part: &str) -> String {
    if part.chars().count() > 2 {
        digits(part, "zero")
    } else {
        number_words(part)
    }
}

/// The part after the point of a version: "12" twelve, "04" oh four.
pub fn minor(part: &str) -> String {
    if part.starts_with('0') {
        digits(part, "oh")
    } else {
        cardinal(int(part))
    }
}

/// A number in a code that is said as a number: one or two digits, or a round one.
pub fn short_number(text: &str) -> bool {
    if text.starts_with('0') {
        return false;
    }
    let n = text.chars().count();
    n <= 2 || text.chars().skip(1).all(|c| c == '0')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_are_said_the_way_dizione_says_them() {
        assert_eq!(cardinal(1234), "one thousand two hundred thirty four");
        assert_eq!(ordinal(21), "twenty first");
        assert_eq!(ordinal(40), "fortieth");
        assert_eq!(year(2005), "two thousand five");
        assert_eq!(year(1984), "nineteen eighty four");
        assert_eq!(pairs(4070), "forty seventy");
        assert_eq!(number_words("3.14"), "three point one four");
        assert_eq!(number_words("007"), "zero zero seven");
        assert_eq!(
            duration(2, 3, 45),
            "two hours, three minutes and forty five seconds"
        );
        assert_eq!(plural_of("nineteen ninety"), "nineteen nineties");
        assert_eq!(fraction_words(1, 8, true), "an eighth");
    }
}
