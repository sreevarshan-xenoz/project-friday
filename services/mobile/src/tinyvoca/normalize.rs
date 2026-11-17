//! Text -> the words to say and the punctuation marks between them (a port of
//! dizione's `normalize.py`, Apache-2.0). Everything that is not a plain word
//! is turned into words here, by rule: numbers, ordinals, money, dates, times,
//! telephone numbers, units, abbreviations, initialisms, symbols, chat
//! spellings, version numbers, codes, web addresses and file names.

use super::convert::{convert, from_text, is_arpabet};
use super::numbers::*;
use super::symbols::{Phone, PUNCTUATION};
use super::tables::{mapping, rows};
use fancy_regex::{Captures, Regex};
use std::collections::{HashMap, HashSet};

type Caps<'a> = Captures<'a>;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Word {
    /// Letters and apostrophes, lower case.
    pub text: String,
    /// Continues the previous word (hyphenated compound, parts of one token).
    pub attach: bool,
    /// As written.
    pub raw: String,
    /// Read letter by letter.
    pub spell: bool,
    /// Pronunciation given inline by the user.
    pub phones: Option<Vec<Phone>>,
}

impl Word {
    fn new(text: &str, raw: &str) -> Self {
        Self {
            text: text.to_string(),
            raw: raw.to_string(),
            ..Default::default()
        }
    }
    fn spelled(text: &str, raw: &str) -> Self {
        Self {
            spell: true,
            ..Self::new(text, raw)
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Word(Word),
    Punct(String),
}

// Private-use characters that carry decisions through the rewriting of the text.
const SPELL: char = '\u{E000}';
const END: char = '\u{E001}';
const FORMULA_SIGN: char = '\u{E004}';
const IS: char = '\u{E006}';
const INLINE: char = '\u{E003}';
const INLINE_BASE: u32 = 0xE100;

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const MONTH_PATTERN: &str = "January|February|March|April|May|June|July|August|September|October|November|December|Jan|Feb|Mar|Apr|Jun|Jul|Aug|Sept|Sep|Oct|Nov|Dec";
const DAY_PATTERN: &str = "Monday|Tuesday|Wednesday|Thursday|Friday|Saturday|Sunday|Thurs|Thur|Tues|Mon|Tue|Wed|Thu|Fri|Sat|Sun";
const CURRENCY_CODES: &str = "USD|EUR|GBP|JPY|INR|CHF|CAD|AUD|CNY";
const ZONES: &str =
    "ET|CT|MT|PT|EST|EDT|CST|CDT|MST|MDT|PST|PDT|GMT|UTC|BST|CET|CEST|IST|AEST|AEDT|JST";
const SIZES: &str = "XXXL|XXL|XXS|XL|XS|S|M|L";
const LENGTHS: &str = "mm|cm|km|m|ft|in|inch|inches|feet|foot|metres?|meters?|yards?|yd|px|pixels?";
const VERSIONED: &str = "version|ver|release|firmware|patch|kernel|build|python|node|nodejs|ios|ipados|macos|watchos|tvos|android|chrome|firefox|safari|java|php|ruby|rust|swift|kotlin|golang|django|rails|react|angular|vue|docker|kubernetes|ubuntu|debian|fedora|windows|linux|postgres|postgresql|mysql|redis|numpy|pytorch|tensorflow|cuda|xcode|gcc|clang|unity|minecraft|typescript|bluetooth|usb|hdmi|opengl|vulkan|directx|upgrade[ds]?|update[ds]?|downgrade[ds]?";
const HALF_DAY: &str = r"(?:\s?([AaPp])\.[Mm]\.?|\s?([AaPp])[Mm]\b)";
const AMOUNT: &str = r"(?P<whole>\d{1,3}(?:,\d{3})+|\d+)(?:\.(?P<fraction>\d+))?";
const EMOJI: &str = "[\u{1F000}-\u{1FAFF}\u{2600}-\u{27BF}\u{1F1E6}-\u{1F1FF}\u{2190}-\u{21FF}\u{2B00}-\u{2BFF}\u{FE0F}\u{200D}\u{20E3}]+";

fn set(words: &str) -> HashSet<&str> {
    words.split_whitespace().collect()
}

macro_rules! rx {
    ($e:expr) => {{
        static R: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
        R.get_or_init(|| Regex::new(&$e).unwrap_or_else(|e| panic!("regex {:?}: {e}", &$e)))
    }};
}

macro_rules! words {
    ($name:ident, $text:expr) => {
        fn $name() -> &'static HashSet<&'static str> {
            static S: std::sync::OnceLock<HashSet<&'static str>> = std::sync::OnceLock::new();
            S.get_or_init(|| set($text))
        }
    };
}

words!(clause_openers, "if elif while when whenever where whether that because since as and or but so then think thinks thought know knows say says said suppose assume means implies hence thus therefore although though unless until once else also clearly obviously indeed here now again first next finally similarly otherwise however moreover");
words!(sentence_starters, "the he she it i we they this that there but and in on at a an if when what why how who my our you then so now after before however");
words!(year_context, "in since by from until till before after during around circa of year early late mid summer winter spring fall autumn class");
words!(cardinal_after, "war chapter part book volume act scene section phase type class level stage title article appendix world");
words!(labelled, "plan vitamin grade type option class section part team group exhibit appendix point model series hepatitis level");
words!(
    leading_apostrophe,
    "'em 'tis 'twas 'til 'cause 'bout 'round 'n"
);
words!(
    phone_words,
    "call dial ring phone telephone tel mobile cell fax hotline text sms whatsapp"
);
words!(phone_links, "to on at or us me him her them back");
words!(
    pin_words,
    "code pin extension ext passcode password zip postcode cvv cvc otp"
);
words!(
    strong_words,
    "number id reference ref serial account tracking booking confirmation"
);
words!(weak_words, "order ticket invoice case claim policy receipt transaction parcel shipment container payment reservation membership customer batch lot item job issue bug request");
words!(
    link_words,
    "is was are were ends ending in with starts starting begins beginning"
);
words!(not_counted, "is was has does goes ships arrives departs leaves shows needs blocks remains includes contains says reads starts ends closes opens runs takes comes this his its us as yes plus minus less");
words!(numbered, "ref reference serial account order invoice tracking part model phone tel fax id room flight case ticket policy licence license booking customer vat tax reg registration");
words!(date_before, "on dated due until till by since from before after for shipped delivered arriving arrives arrive arrived expires expired ends ending starts starting begins beginning monday tuesday wednesday thursday friday saturday sunday mon tue tues wed thu thur thurs fri sat sun");
words!(
    ratio_words,
    "ratio ratios scale odds aspect mix split proportion"
);
words!(
    duration_words,
    "lasted took duration runtime recording video clip track lap pace length long"
);
words!(clock_words, "window slot open opens opening hours shift session served service available booked appointment delivery arrive arrives arriving");
words!(capital_words, "IN ON AT TO OF BY OR IS AS IF NO SO WE HE MY ME DO GO UP AN AM PM BE IT US THE AND FOR BUT NOT ALL");
words!(
    short_words,
    "my to in on of go up no do me we us it is at by be so or an as if he hi ok"
);
words!(
    initialisms,
    "US IT AI ID UN EU UK LA AD BC AM PM OS PE PA MA LED SAT RIP ER"
);
words!(sounds, "sh shh ss zz hm hmm mm tsk psst pfft hmph psh pssh");

fn month_number(name: &str) -> usize {
    let l: String = name.to_lowercase().chars().take(3).collect();
    MONTHS
        .iter()
        .position(|m| m.to_lowercase().starts_with(&l))
        .map(|i| i + 1)
        .unwrap_or(1)
}

fn day_name(key: &str) -> Option<&'static str> {
    Some(match key {
        "mon" => "Monday",
        "tue" => "Tuesday",
        "wed" => "Wednesday",
        "thu" => "Thursday",
        "fri" => "Friday",
        "sat" => "Saturday",
        "sun" => "Sunday",
        _ => return None,
    })
}

const CURRENCY: [(&str, [&str; 4]); 14] = [
    ("$", ["dollar", "dollars", "cent", "cents"]),
    ("£", ["pound", "pounds", "penny", "pence"]),
    ("€", ["euro", "euros", "cent", "cents"]),
    ("¥", ["yen", "yen", "", ""]),
    ("₹", ["rupee", "rupees", "paisa", "paise"]),
    ("USD", ["dollar", "dollars", "cent", "cents"]),
    ("EUR", ["euro", "euros", "cent", "cents"]),
    ("GBP", ["pound", "pounds", "penny", "pence"]),
    ("JPY", ["yen", "yen", "", ""]),
    ("INR", ["rupee", "rupees", "paisa", "paise"]),
    (
        "CHF",
        ["Swiss franc", "Swiss francs", "centime", "centimes"],
    ),
    (
        "CAD",
        ["Canadian dollar", "Canadian dollars", "cent", "cents"],
    ),
    (
        "AUD",
        ["Australian dollar", "Australian dollars", "cent", "cents"],
    ),
    ("CNY", ["yuan", "yuan", "", ""]),
];

fn currency(symbol: &str) -> [&'static str; 4] {
    CURRENCY
        .iter()
        .find(|(s, _)| *s == symbol)
        .map(|(_, v)| *v)
        .unwrap_or(CURRENCY[0].1)
}

fn scale_word(letter: &str) -> String {
    match letter.to_lowercase().as_str() {
        "k" => "thousand".into(),
        "m" => "million".into(),
        "b" | "bn" => "billion".into(),
        "t" => "trillion".into(),
        other => other.to_string(),
    }
}

fn symbol_word(c: char) -> Option<&'static str> {
    Some(match c {
        '&' => "and",
        '@' => "at",
        '+' => "plus",
        '=' => "equals",
        '%' => "percent",
        '×' => "times",
        '÷' => "divided by",
        '±' => "plus or minus",
        '©' => "copyright",
        '§' => "section",
        '°' => "degrees",
        '≥' => "greater than or equal to",
        '≤' => "less than or equal to",
        '≠' => "not equal to",
        '≈' => "approximately",
        '∞' => "infinity",
        '√' => "square root of",
        'π' => "pi",
        _ => return None,
    })
}

fn comparison_word(sign: &str) -> &'static str {
    match sign {
        ">=" => "greater than or equal to",
        "<=" => "less than or equal to",
        _ => "not equal to",
    }
}

fn separator(piece: &str) -> &'static str {
    match piece {
        "." => "dot",
        "/" => "slash",
        "@" => "at",
        "-" => "dash",
        "_" => "underscore",
        ":" => "colon",
        "=" => "equals",
        "&" => "and",
        "+" => "plus",
        _ => "",
    }
}

fn roman(raw: &str) -> Option<i128> {
    Some(match raw {
        "II" => 2,
        "III" => 3,
        "IV" => 4,
        "VI" => 6,
        "VII" => 7,
        "VIII" => 8,
        "IX" => 9,
        "XI" => 11,
        "XII" => 12,
        "XIII" => 13,
        "XIV" => 14,
        "XV" => 15,
        "XVI" => 16,
        "XVIII" => 18,
        "XX" => 20,
        _ => return None,
    })
}

const ACCENT_DECIDES: [(&str, &str); 8] = [
    ("résumé", "R EH1 Z AH0 M EY2"),
    ("resumé", "R EH1 Z AH0 M EY2"),
    ("exposé", "EH2 K S P OW0 Z EY1"),
    ("rosé", "R OW0 Z EY1"),
    ("pâté", "P AA0 T EY1"),
    ("maté", "M AA1 T EY2"),
    ("saké", "S AA1 K EY2"),
    ("lamé", "L AA0 M EY1"),
];

fn letter_form(c: char) -> Option<&'static str> {
    Some(match c {
        'ß' => "ss",
        'æ' => "ae",
        'Æ' => "Ae",
        'œ' => "oe",
        'Œ' => "Oe",
        'ø' => "o",
        'Ø' => "O",
        'đ' => "d",
        'ł' => "l",
        'Ł' => "L",
        _ => return None,
    })
}

/// An accented Latin letter without its accent; other characters are left alone.
fn plain_letter(c: char) -> String {
    use unicode_normalization::char::is_combining_mark;
    use unicode_normalization::UnicodeNormalization;
    if c.is_ascii() || !c.is_alphabetic() {
        return c.to_string();
    }
    if let Some(f) = letter_form(c) {
        return f.to_string();
    }
    let base: String = std::iter::once(c)
        .nfkd()
        .filter(|x| !is_combining_mark(*x))
        .collect();
    if !base.is_empty() && base.chars().all(|x| x.is_ascii_alphabetic()) {
        base
    } else {
        c.to_string()
    }
}

// ---- small string helpers ---------------------------------------------------

fn first_char(s: &str) -> char {
    s.chars().next().unwrap_or('\0')
}
fn last_chars(s: &str, n: usize) -> &str {
    let count = s.chars().count();
    if count <= n {
        return s;
    }
    let skip = count - n;
    &s[s.char_indices().nth(skip).map(|(i, _)| i).unwrap_or(0)..]
}
/// Python's `str.isupper()`: some cased letter, none of them lower case.
fn is_upper(s: &str) -> bool {
    s.chars()
        .any(|c| c.is_alphabetic() && (c.is_uppercase() || c.is_lowercase()))
        && !s.chars().any(|c| c.is_lowercase())
}
fn is_lower(s: &str) -> bool {
    s.chars().any(|c| c.is_lowercase()) && !s.chars().any(|c| c.is_uppercase())
}
fn is_alpha(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_alphabetic())
}
fn is_digits(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_digit())
}
fn is_ascii_alpha_char(c: char) -> bool {
    c.is_ascii_alphabetic()
}
fn group<'a>(c: &'a Caps, n: usize) -> &'a str {
    c.get(n).map(|m| m.as_str()).unwrap_or("")
}
fn named<'a>(c: &'a Caps, n: &str) -> &'a str {
    c.name(n).map(|m| m.as_str()).unwrap_or("")
}
fn whole(c: &Caps) -> String {
    c[0].to_string()
}
fn spelled(letters: &str) -> String {
    format!("{SPELL}{letters}{END}")
}

/// `re.sub` with a function; matches are taken from the original text.
fn sub(re: &Regex, text: &str, mut f: impl FnMut(&Caps) -> String) -> String {
    let mut out = String::with_capacity(text.len() + 16);
    let mut last = 0;
    for c in re.captures_iter(text) {
        let Ok(c) = c else { break };
        let m = c.get(0).expect("match");
        out.push_str(&text[last..m.start()]);
        out.push_str(&f(&c));
        last = m.end();
    }
    out.push_str(&text[last..]);
    out
}

fn sub_str(re: &Regex, text: &str, with: &str) -> String {
    sub(re, text, |_| with.to_string())
}

fn search<'t>(re: &Regex, text: &'t str) -> Option<Captures<'t>> {
    re.captures(text).ok().flatten()
}

fn is_match(re: &Regex, text: &str) -> bool {
    re.is_match(text).unwrap_or(false)
}

fn find_all(re: &Regex, text: &str) -> Vec<String> {
    re.find_iter(text)
        .filter_map(|m| m.ok())
        .map(|m| m.as_str().to_string())
        .collect()
}

/// The words just before `position` in the same clause, in lower case.
fn before_words(text: &str, position: usize, count: usize) -> Vec<String> {
    let start = text[..position]
        .char_indices()
        .rev()
        .nth(79)
        .map(|(i, _)| i)
        .unwrap_or(0);
    let window = &text[start..position];
    let clause = rx!(r#"[.!?;…]\s|[()"]"#)
        .find_iter(window)
        .filter_map(|m| m.ok())
        .last()
        .map(|m| &window[m.end()..])
        .unwrap_or(window);
    let words: Vec<String> = find_all(rx!("[A-Za-z]+"), clause)
        .into_iter()
        .map(|w| w.to_lowercase())
        .collect();
    words[words.len().saturating_sub(count)..].to_vec()
}

fn identifier(text: &str, position: usize) -> Option<&'static str> {
    let words = before_words(text, position, 4);
    if words.is_empty() {
        return None;
    }
    let last = words.last().unwrap().as_str();
    let middle = words[words.len().saturating_sub(3)..words.len() - 1].iter();
    if phone_words().contains(last)
        || (phone_links().contains(last)
            && middle.clone().any(|w| phone_words().contains(w.as_str())))
    {
        return Some("phone");
    }
    let mut index = words.len() - 1;
    while index > 0 && link_words().contains(words[index].as_str()) {
        index -= 1;
    }
    for (kind, group) in [
        ("phone", phone_words()),
        ("pin", pin_words()),
        ("strong", strong_words()),
        ("weak", weak_words()),
    ] {
        if group.contains(words[index].as_str()) {
            return Some(kind);
        }
    }
    if last == "enter" {
        Some("enter")
    } else {
        None
    }
}

fn decimal_comma(text: &str) -> String {
    if is_match(rx!(r"^\d+,\d{1,2}$"), text) {
        text.replace(',', ".")
    } else {
        text.to_string()
    }
}

/// Digits inside a file name or an address: 3, 2026, 0412.
fn address_digits(text: &str) -> String {
    let n = text.chars().count();
    if n <= 2 && !(n == 2 && text.starts_with('0')) {
        return cardinal(int(text));
    }
    if n == 4 && (1900..=2099).contains(&int(text)) {
        return year(int(text));
    }
    digits(text, "zero")
}

/// `text.replace` of the characters `from` by the string `to`, one at a time.
fn replace_all(text: &str, pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .fold(text.to_string(), |t, (a, b)| t.replace(a, b))
}

// ---- patterns that need constants ------------------------------------------

fn compared() -> &'static Regex {
    rx!(r"(?P<gap>[ \t]?)(?P<sign>>=|<=|!=|[<>≥≤≠])\k<gap>")
}
fn value_before() -> &'static Regex {
    rx!(r"(?:(?<![\w.,])\d[\d.,]*%?|(?<![\w'’.-])[A-Za-z]|[)\]])$")
}
fn value_after() -> &'static Regex {
    rx!(r"\A(?:[-+]?[$£€¥₹]?\d[\d.,]*%?(?!\w)|\.\d|[A-Za-z](?![\w'’])|[(\[])")
}

fn letter_places() -> &'static Vec<Regex> {
    static P: std::sync::OnceLock<Vec<Regex>> = std::sync::OnceLock::new();
    P.get_or_init(|| {
        let sign = r"[<>+*/^≤≥≠≈×÷±]";
        let spaced = r" [<>+*≤≥≠≈×÷±] ";
        let no_phrase = r"(?![\w'’-]|\s+(?!(?:and|or|then|if|when|where|while|for|is|are|was|but|so|in|to)\b)[A-Za-z])";
        [
            format!(r"(?<![\w'’.-])(?P<letter>[Aakur])(?={IS}?\s?[<>!=]?=)"),
            format!(r"[<>!=]?=\s?(?P<letter>[Aakur]){no_phrase}"),
            format!(r"(?<![\w'’.-])(?P<letter>[Aakur])(?={IS}?{sign}(?:[\w($]|\.\d))"),
            format!(r"(?<=[\w){IS}]{sign})(?<![Ww]/)(?P<letter>[Aakur])(?![\w'’-])"),
            format!(r"(?<![\w'’.-])(?P<letter>[Aakur])(?={IS}?{spaced}(?:[A-Za-z]|\d[\d.,]*)(?![\w'’]))"),
            format!(r"(?<![\w'’.])(?:[A-Za-z]|\d[\d.,]*){IS}?{spaced}(?P<letter>[Aakur]){no_phrase}"),
            r"(?<=\()(?P<letter>[Aakur])(?=\)|, )".to_string(),
            r"(?<=, )(?P<letter>[Aakur])(?=\))".to_string(),
            r"(?:(?<=\s)|^)(?P<letter>[akur])(?=\)\s)".to_string(),
        ]
        .iter()
        .map(|p| Regex::new(p).unwrap_or_else(|e| panic!("letter place {p:?}: {e}")))
        .collect()
    })
}

/// `text` with the angle brackets that enclose something replaced by a mark,
/// one byte for one, the inner ones first.
fn without_angles(text: &str) -> String {
    let angles = rx!(format!(
        r"(?<![\w){END}{IS}])<[A-Za-z0-9/!?][^<>(){{}}+*;|\n]*>|(?<=[\w{END}{IS}])<[^<>(){{}}+*;|\n]{{1,80}}>(?![\w$])|</?[A-Za-z][\w-]+/?>"
    ));
    let mut text = text.to_string();
    while text.contains('<') && text.contains('>') {
        let marked = sub(angles, &text, |c| {
            let m = &c[0];
            format!("\u{5}{}\u{5}", &m[1..m.len() - 1])
        });
        if marked == text {
            break;
        }
        text = marked;
    }
    text
}

fn joined_comparison() -> &'static Regex {
    rx!(format!(
        r"(?<=[\w){END}{IS}])[<>](?=[\w({SPELL}]|\.\d| minus \d)"
    ))
}

// ---- the normalizer -----------------------------------------------------------

pub struct Normalizer {
    abbreviations: HashMap<String, (String, String)>,
    units: HashMap<String, (String, String)>,
    exact_units: HashMap<String, (String, String)>,
    chat: HashMap<String, String>,
    replies: HashSet<String>,
    endings: HashMap<String, String>,
    unit_names: String,
    unit: Regex,
    area: Regex,
    area_plain: Regex,
}

const ABBREVIATIONS: &str = include_str!("data/abbreviations.txt");
const UNITS: &str = include_str!("data/units.txt");
const CHAT: &str = include_str!("data/chat.txt");
const ENDINGS: &str = include_str!("data/endings.txt");

impl Normalizer {
    pub fn new() -> Result<Self, String> {
        let mut abbreviations = HashMap::new();
        for (n, c) in rows(ABBREVIATIONS) {
            if c.len() < 3 {
                return Err(format!("abbreviations.txt line {n}"));
            }
            abbreviations.insert(c[0].clone(), (c[1].clone(), c[2].clone()));
        }
        let (mut units, mut exact_units) = (HashMap::new(), HashMap::new());
        for (n, c) in rows(UNITS) {
            if c.len() < 3 {
                return Err(format!("units.txt line {n}"));
            }
            if c.len() > 3 && c[3] == "exact" {
                exact_units.insert(c[0].clone(), (c[1].clone(), c[2].clone()));
            } else {
                units.insert(c[0].to_lowercase(), (c[1].clone(), c[2].clone()));
            }
        }
        let (mut chat, mut replies) = (HashMap::new(), HashSet::new());
        for (n, c) in rows(CHAT) {
            if c.len() > 2 && c[2..] != ["alone".to_string()] {
                return Err(format!(
                    "chat.txt line {n}: after the words to say there can only be \"alone\""
                ));
            }
            chat.insert(
                c[0].clone(),
                c.get(1).cloned().unwrap_or_else(|| "-".into()),
            );
            if c.len() > 2 {
                replies.insert(c[0].clone());
            }
        }
        let mut names: Vec<String> = units
            .keys()
            .chain(exact_units.keys())
            .map(|u| fancy_regex::escape(u).into_owned())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        names.sort_by(|a, b| b.chars().count().cmp(&a.chars().count()).then(a.cmp(b)));
        let unit_names = names.join("|");
        let number = r"\d[\d,]*(?:\.\d+)?";
        let unit = Regex::new(&format!(
            r"(?i)(?<![\w.])({number})(?:[-–]({number}))?\s?({unit_names})(?![A-Za-z])(\.(?=\s+(?-i:[a-z])))?"
        ))
        .map_err(|e| e.to_string())?;
        let area = Regex::new(&format!(
            r"(?<![\w.])({number})\s?(km|cm|mm|m|ft|mi|yd|in)([²³])"
        ))
        .map_err(|e| e.to_string())?;
        let area_plain = Regex::new(&format!(
            r"(?<![\w.])({number})\s?(km|cm|mm|m|ft)([23])(?![\w])"
        ))
        .map_err(|e| e.to_string())?;
        Ok(Self {
            abbreviations,
            units,
            exact_units,
            chat,
            replies,
            endings: mapping(ENDINGS),
            unit_names,
            unit,
            area,
            area_plain,
        })
    }

    /// Words and punctuation marks of `text`. `is_word(spelling)` tells whether
    /// the dictionary knows a spelling (used for capitals and chat forms).
    pub fn tokens(&self, text: &str, is_word: &dyn Fn(&str) -> bool) -> Vec<Token> {
        Run {
            n: self,
            is_word,
            inline: Vec::new(),
            day_first: false,
            dated: false,
        }
        .tokens(text)
    }
}

struct Run<'a> {
    n: &'a Normalizer,
    is_word: &'a dyn Fn(&str) -> bool,
    inline: Vec<(String, Vec<Phone>)>,
    day_first: bool,
    dated: bool,
}

impl Run<'_> {
    fn is_word(&self, w: &str) -> bool {
        (self.is_word)(w)
    }

    // -- pieces of the text that are not words --

    fn amount(&self, symbol: &str, whole: &str, fraction: &str, scale: &str) -> String {
        let [one, many, cent, cents] = currency(symbol);
        let with_fraction = if fraction.is_empty() {
            whole.to_string()
        } else {
            format!("{whole}.{fraction}")
        };
        if !scale.is_empty() {
            return format!(
                "{} {} {many}",
                number_words(&with_fraction),
                scale_word(scale)
            );
        }
        if !fraction.is_empty() && (fraction.chars().count() > 2 || cent.is_empty()) {
            return format!("{} {many}", number_words(&with_fraction));
        }
        let amount = int(whole);
        let frac = if fraction.is_empty() {
            "0".to_string()
        } else {
            format!("{fraction:0<2}")
        };
        let change = int(&frac);
        let mut parts = Vec::new();
        if amount != 0 || change == 0 {
            parts.push(format!(
                "{} {}",
                cardinal(amount),
                if amount == 1 { one } else { many }
            ));
        }
        if change != 0 && !cent.is_empty() {
            parts.push(format!(
                "{} {}",
                cardinal(change),
                if change == 1 { cent } else { cents }
            ));
        }
        parts.join(" and ")
    }

    /// "1.299,50" as "1,299.50". Four groups that could be an internet address are left alone.
    fn continental(c: &Caps) -> String {
        let groups: Vec<&str> = group(c, 1).split('.').collect();
        if groups.len() == 4 && groups.iter().all(|g| int(g) <= 255) {
            return whole(c);
        }
        format!("{}.{}", groups.join(","), group(c, 2))
    }

    fn money(&self, c: &Caps) -> String {
        let w = named(c, "whole").replace([',', ' '], "");
        let words = self.amount(
            named(c, "symbol"),
            &w,
            named(c, "fraction"),
            named(c, "scale"),
        );
        let minus = !named(c, "minus").is_empty() || !named(c, "inner").is_empty();
        format!("{} {words} ", if minus { " minus" } else { "" })
    }

    /// " A M" or " P M" as letters. A full stop that closes "p.m." is kept as a
    /// sentence end only when a new sentence follows.
    fn half_day(text: &str, end: usize, whole_match: &str, dotted: &str, plain: &str) -> String {
        if dotted.is_empty() && plain.is_empty() {
            return String::new();
        }
        let following = text[end..].trim_start();
        let first = search(rx!(r"^[A-Za-z']+"), following).map(|c| c[0].to_lowercase());
        let closes = !dotted.is_empty()
            && whole_match.ends_with('.')
            && (following.is_empty()
                || (first_char(following).is_uppercase()
                    && first.is_some_and(|f| sentence_starters().contains(f.as_str()))));
        let letter = if dotted.is_empty() { plain } else { dotted }.to_uppercase();
        format!("{SPELL}{letter}M{END}{} ", if closes { " ." } else { "" })
    }

    fn time(&self, c: &Caps, text: &str) -> String {
        let m = c.get(0).unwrap();
        let (hour, minute) = (int(group(c, 1)), int(group(c, 2)));
        let second = c.get(3).map(|m| m.as_str());
        let half_dotted = group(c, 4);
        let half_plain = group(c, 5);
        let half = !half_dotted.is_empty() || !half_plain.is_empty();
        let before = before_words(text, m.start(), 3);
        if let (Some(second), false) = (second, half) {
            if !before.last().is_some_and(|b| b == "at") {
                return format!(" {} ", duration(hour, minute, int(second)));
            }
        }
        if hour > 24 && second.is_none() && !half {
            let after =
                search(rx!(r"^\s*([A-Za-z]+)"), &text[m.end()..]).map(|c| c[1].to_lowercase());
            if before.iter().any(|w| duration_words().contains(w.as_str()))
                || after.is_some_and(|a| duration_words().contains(a.as_str()))
            {
                return format!(" {} ", duration(0, hour, minute));
            }
            return format!(
                " {} {}{} ",
                cardinal(hour),
                if minute < 10 { "oh " } else { "" },
                cardinal(minute)
            );
        }
        if hour > 24 {
            return whole(c);
        }
        let mut words = cardinal(hour);
        if minute == 0 {
            if half {
            } else if hour == 0 {
                words = "midnight".into();
            } else if hour > 12 {
                words += " hundred";
            } else {
                words += " o'clock";
            }
        } else {
            words += if minute < 10 { " oh " } else { " " };
            words += &cardinal(minute);
        }
        if let Some(s) = second.filter(|s| !s.is_empty()) {
            words += &format!(
                " and {} {}",
                cardinal(int(s)),
                if int(s) == 1 { "second" } else { "seconds" }
            );
        }
        format!(
            " {words} {}",
            Self::half_day(text, m.end(), m.as_str(), half_dotted, half_plain)
        )
    }

    /// "9.30 to 11.30" where the words before say it is a time of day.
    fn dot_times(&self, c: &Caps, text: &str) -> String {
        let m = c.get(0).unwrap();
        let following = &text[m.end()..];
        let units = rx!(format!(
            r"(?i)^\s?(?:%|percent|million|billion|{})(?![A-Za-z])",
            self.n.unit_names
        ));
        if int(group(c, 1)) > 23
            || int(group(c, 4)) > 23
            || !before_words(text, m.start(), 4)
                .iter()
                .any(|w| clock_words().contains(w.as_str()))
            || is_match(units, following)
        {
            return whole(c);
        }
        format!(
            "{}:{}{}{}:{}",
            group(c, 1),
            group(c, 2),
            group(c, 3),
            group(c, 4),
            group(c, 5)
        )
    }

    /// 16:9, 3:1, 1:25,000. Two digits after the colon are a time unless the words say "ratio".
    fn ratio(&self, c: &Caps, text: &str) -> String {
        let m = c.get(0).unwrap();
        let (first, second) = (group(c, 1), group(c, 2));
        if first.chars().count() <= 2
            && second.chars().count() == 2
            && "012345".contains(first_char(second))
        {
            let after =
                search(rx!(r"^\s*([A-Za-z]+)"), &text[m.end()..]).map(|c| c[1].to_lowercase());
            if !(before_words(text, m.start(), 3)
                .iter()
                .any(|w| ratio_words().contains(w.as_str()))
                || after.is_some_and(|a| ratio_words().contains(a.as_str())))
            {
                return whole(c);
            }
        }
        format!(" {} to {} ", number_words(first), number_words(second))
    }

    fn date(month: i128, day: i128, year_text: &str, the: bool) -> Option<String> {
        if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
            return None;
        }
        let name = MONTHS[month as usize - 1];
        let mut words = if the {
            format!("the {} of {name}", ordinal(day))
        } else {
            format!("{name} {}", ordinal(day))
        };
        if !year_text.is_empty() {
            let spoken = if year_text.chars().count() == 4 {
                year(int(year_text))
            } else {
                format!(
                    "{}{}",
                    if int(year_text) < 10 { "oh " } else { "" },
                    cardinal(int(year_text))
                )
            };
            words += &format!(" {spoken}");
        }
        Some(format!(" {words} "))
    }

    fn slash_date(&self, c: &Caps) -> String {
        let (mut first, mut second) = (int(group(c, 1)), int(group(c, 2)));
        if (first > 12 && 12 >= second) || (self.day_first && second <= 12) {
            std::mem::swap(&mut first, &mut second);
        }
        Self::date(first, second, group(c, 3), false).unwrap_or_else(|| whole(c))
    }

    /// 15.11.2026: dates written with full stops put the day first.
    fn dot_date(&self, c: &Caps) -> String {
        let (mut day, mut month) = (int(group(c, 1)), int(group(c, 2)));
        if month > 12 && 12 >= day {
            std::mem::swap(&mut day, &mut month);
        }
        Self::date(month, day, group(c, 3), false).unwrap_or_else(|| whole(c))
    }

    /// "22 February", "5 Jan. 2026". The stop after a short month name is dropped; any other one ends the sentence.
    fn day_month(&self, c: &Caps) -> String {
        let month = group(c, 2);
        match Self::date(
            month_number(month) as i128,
            int(group(c, 1)),
            group(c, 4),
            true,
        ) {
            None => whole(c),
            Some(spoken) => {
                spoken
                    + if !group(c, 3).is_empty() && MONTHS.contains(&month) {
                        ". "
                    } else {
                        ""
                    }
            }
        }
    }

    /// "3-17 November" and "November 3-17".
    fn days_of_month(&self, c: &Caps) -> String {
        let month_first = first_char(group(c, 1)).is_alphabetic();
        let (month, a, b) = if month_first {
            (group(c, 1), group(c, 2), group(c, 3))
        } else {
            (group(c, 3), group(c, 1), group(c, 2))
        };
        let (first, second) = (int(a), int(b));
        if !(1..=31).contains(&first) || !(1..=31).contains(&second) {
            return whole(c);
        }
        let name = MONTHS[month_number(month) - 1];
        let words = if month_first {
            format!("{name} {} to {}", ordinal(first), ordinal(second))
        } else {
            format!(
                "the {} to the {} of {name}",
                ordinal(first),
                ordinal(second)
            )
        };
        let tail = if group(c, 4).is_empty() {
            String::new()
        } else {
            format!(" {}", year(int(group(c, 4))))
        };
        format!(" {words}{tail} ")
    }

    /// "Mon.-Fri.", "Jan-Mar": from one day or month to another.
    fn day_range(&self, c: &Caps, text: &str) -> String {
        let m = c.get(0).unwrap();
        let names: Vec<String> = [group(c, 1), group(c, 3)]
            .iter()
            .map(|name| {
                let key = name.to_lowercase();
                if let Some((_, w)) = self.n.abbreviations.get(&key) {
                    w.clone()
                } else {
                    day_name(&key)
                        .map(String::from)
                        .unwrap_or_else(|| name.to_string())
                }
            })
            .collect();
        let following = text[m.end()..].trim_start();
        let first = search(rx!(r"^[A-Za-z']+"), following).map(|c| c[0].to_lowercase());
        let ends = !group(c, 4).is_empty()
            && (following.is_empty()
                || (first_char(following).is_uppercase()
                    && first.is_some_and(|f| sentence_starters().contains(f.as_str()))));
        format!(
            "{} to {}{}",
            names[0],
            names[1],
            if ends { " ." } else { "" }
        )
    }

    /// "14/03", "12/10" as a day of the year, or None when it is a fraction.
    fn short_date(&self, c: &Caps, text: &str) -> Option<String> {
        let m = c.get(0).unwrap();
        let (first_text, second_text) = (group(c, 1), group(c, 2));
        if first_text.chars().count() > 2 || second_text.chars().count() > 2 {
            return None;
        }
        let (mut first, mut second) = (int(first_text), int(second_text));
        let before = before_words(text, m.start(), 4);
        if (1..=12).contains(&first)
            && (20..=49).contains(&second)
            && before.iter().any(|w| w == "card")
        {
            return Some(format!(
                " {} {} ",
                MONTHS[first as usize - 1],
                year(2000 + second)
            ));
        }
        let last = before.last().map(String::as_str).unwrap_or("");
        let classic = first < second && [2, 3, 4, 5, 6, 8, 10, 16, 32, 64, 100].contains(&second);
        let zero = (first_text.chars().count() == 2 && first_text.starts_with('0'))
            || (second_text.chars().count() == 2 && second_text.starts_with('0'));
        if !(zero
            || (12 < first && first <= 31 && second <= 12 && (first, second) != (24, 7))
            || (date_before().contains(last) && (!classic || last == "on"))
            || (self.dated && !classic && first != second))
        {
            return None;
        }
        if first > 12 || (self.day_first && second <= 12) {
            std::mem::swap(&mut first, &mut second);
        }
        Self::date(first, second, "", false)
    }

    fn fraction(&self, c: &Caps, text: &str) -> String {
        if let Some(spoken) = self.short_date(c, text) {
            return spoken;
        }
        let (top, bottom) = (int(group(c, 1)), int(group(c, 2)));
        if (top, bottom) == (24, 7) || top == bottom {
            return format!(" {} {} ", cardinal(top), cardinal(bottom));
        }
        if !(0 < top && top < 100) || !(1 < bottom && bottom <= 100) {
            return format!(" {} over {} ", cardinal(top), cardinal(bottom));
        }
        format!(" {} ", fraction_words(top, bottom, false))
    }

    /// "2 1/2": two and a half.
    fn mixed_fraction(&self, c: &Caps) -> String {
        let (top, bottom) = (int(group(c, 2)), int(group(c, 3)));
        if !(0 < top && top < bottom) {
            return whole(c);
        }
        format!(
            " {} and {} ",
            number_words(group(c, 1)),
            fraction_words(top, bottom, true)
        )
    }

    /// "½" and "1½". Runs before the text is normalised, which would turn "1½" into "11/2".
    fn vulgar(&self, c: &Caps) -> String {
        use unicode_normalization::UnicodeNormalization;
        let folded: String = group(c, 2).nfkc().collect();
        let (t, b) = folded.split_once('⁄').unwrap_or(("1", "2"));
        let (top, bottom) = (int(t), int(b));
        match c.get(1) {
            None => format!(" {top}/{bottom} "),
            Some(w) => format!(
                " {} and {} ",
                cardinal(int(w.as_str())),
                fraction_words(top, bottom, true)
            ),
        }
    }

    fn phone(&self, c: &Caps) -> String {
        let groups: Vec<String> = (1..c.len())
            .filter_map(|i| c.get(i))
            .map(|m| m.as_str())
            .filter(|g| !g.is_empty())
            .map(|g| {
                if g == "800" {
                    "eight hundred".to_string()
                } else {
                    digits(g, "oh")
                }
            })
            .collect();
        format!(" {} ", groups.join(", "))
    }

    fn measure(&self, c: &Caps, text: &str) -> String {
        let unit = group(c, 3);
        let end3 = c.get(3).unwrap().end();
        let names = self
            .n
            .exact_units
            .get(unit)
            .or_else(|| self.n.units.get(&unit.to_lowercase()));
        let Some((one, many)) = names else {
            return whole(c);
        };
        if unit.chars().count() == 1
            && matches!(
                text[end3..].chars().next(),
                Some('-') | Some('\'') | Some('/')
            )
        {
            return whole(c);
        }
        let value = decimal_comma(group(c, 1));
        if let Some(upper) = c.get(2).map(|m| decimal_comma(m.as_str())) {
            return format!(
                " {} to {} {many} ",
                number_words(&value),
                number_words(&upper)
            );
        }
        format!(
            " {} {} ",
            number_words(&value),
            if value == "1" || value == "1.0" {
                one
            } else {
                many
            }
        )
    }

    /// "120 m²", "3 m3": square and cubic measures.
    fn square(&self, c: &Caps) -> String {
        let unit = group(c, 2);
        let (one, many) = if unit == "in" {
            ("inch".to_string(), "inches".to_string())
        } else {
            self.n
                .exact_units
                .get(unit)
                .or_else(|| self.n.units.get(unit))
                .cloned()
                .unwrap_or_default()
        };
        let kind = if "²2".contains(group(c, 3)) {
            "square"
        } else {
            "cubic"
        };
        let value = decimal_comma(group(c, 1));
        format!(
            " {} {kind} {} ",
            number_words(&value),
            if value == "1" || value == "1.0" {
                one
            } else {
                many
            }
        )
    }

    /// True when a plural noun follows a number: it is then an amount, not a reference.
    fn counted(&self, text: &str, end: usize) -> bool {
        let Some(after) = search(rx!(r"^\s*([A-Za-z]+)"), &text[end..]) else {
            return false;
        };
        let word = after[1].to_lowercase();
        if ["people", "men", "women", "children", "feet", "miles"].contains(&word.as_str()) {
            return true;
        }
        word.len() > 3
            && word.ends_with('s')
            && !word.ends_with("ss")
            && !not_counted().contains(word.as_str())
            && self.is_word(&word[..word.len() - 1])
    }

    fn number(&self, c: &Caps, text: &str) -> String {
        let m = c.get(0).unwrap();
        let value = m.as_str();
        let plain = value.replace(',', "");
        let plain_len = plain.chars().count();
        if !plain.contains('.') && !value.contains(',') && !plain.starts_with('0') && plain_len > 2
        {
            let kind = identifier(text, m.start());
            let limit = match kind {
                Some("phone") | Some("pin") => 3,
                Some("strong") | Some("enter") => 4,
                Some("weak") => 5,
                _ => 7,
            };
            if plain_len >= limit
                && !(kind.is_none() && plain.ends_with("000"))
                && !(kind == Some("enter")
                    && plain_len == 4
                    && (1900..=2099).contains(&int(&plain)))
                && !(matches!(kind, None | Some("phone") | Some("weak"))
                    && self.counted(text, m.end()))
            {
                return format!(
                    " {} ",
                    digits(&plain, if kind == Some("phone") { "oh" } else { "zero" })
                );
            }
        }
        if !plain.contains('.') && !value.contains(',') && plain_len == 4 && !plain.starts_with('0')
        {
            let number = int(&plain);
            let before = find_all(rx!("[A-Za-z]+"), last_chars(&text[..m.start()], 24));
            let after = search(rx!(r"^\s*([A-Za-z]+)"), &text[m.end()..]);
            let counted = after.is_some_and(|a| {
                is_match(
                    rx!(r"(?:[^s]s|people|men|women|children|feet|miles)$"),
                    &a[1].to_lowercase(),
                )
            });
            let dated = before.last().is_some_and(|b| {
                let l = b.to_lowercase();
                year_context().contains(l.as_str())
                    || MONTHS.iter().any(|mo| {
                        mo.to_lowercase()
                            .starts_with(&l.chars().take(3).collect::<String>())
                            && l.chars().count() >= 3
                    })
            });
            let model = search(rx!(r"(?<![A-Za-z])([A-Z]{2,5}) $"), &text[..m.start()]);
            if model.is_some_and(|mo| !capital_words().contains(&mo[1])) && !counted {
                return format!(" {} ", pairs(number));
            }
            if (1100..=1999).contains(&number)
                || ((2000..=2099).contains(&number) && (dated || !counted))
            {
                return format!(" {} ", year(number));
            }
        }
        format!(" {} ", number_words(value))
    }

    // -- codes and reference numbers --

    /// Letters inside a code that are a word to say, not letters to name: SAVE-20.
    fn reads_as_word(&self, letters: &str) -> bool {
        let lower = letters.to_lowercase();
        if lower.chars().count() < 3 || !self.is_word(&lower) {
            return false;
        }
        lower.chars().count() > 3
            || ["s", "es", "ed", "ing"]
                .iter()
                .any(|e| self.is_word(&format!("{lower}{e}")))
    }

    /// A group that mixes letters and digits: the letters are named, the digits read one by one.
    fn alnum(group: &str, zero: &str) -> String {
        find_all(rx!(r"[A-Za-z]+|\d+"), group)
            .iter()
            .map(|run| {
                if is_digits(run) {
                    digits(run, zero)
                } else {
                    spelled(run)
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Groups of digits with a short break between them. Small numbers stay numbers: 9-1-1.
    fn digit_groups(groups: &[&str], zero: &str) -> String {
        if groups
            .iter()
            .all(|g| short_number(g) && g.chars().count() <= 2)
        {
            return groups
                .iter()
                .map(|g| cardinal(int(g)))
                .collect::<Vec<_>>()
                .join(" , ");
        }
        groups
            .iter()
            .map(|g| digits(g, zero))
            .collect::<Vec<_>>()
            .join(" , ")
    }

    fn code_words(&self, groups: &[&str], c: &Caps, text: &str) -> Option<String> {
        let token = groups.join("-");
        if !token.chars().any(|ch| ch.is_ascii_digit()) {
            return None; // well-known, Wi-Fi
        }
        let kinds: Vec<char> = groups
            .iter()
            .map(|g| {
                if is_digits(g) {
                    'd'
                } else if g.chars().any(|ch| ch.is_ascii_digit()) {
                    'm'
                } else if is_upper(g) {
                    'u'
                } else {
                    'l'
                }
            })
            .collect();
        if kinds.iter().all(|k| *k == 'd') {
            if groups.len() == 2 {
                let years = groups
                    .iter()
                    .all(|g| g.chars().count() == 4 && (1000..=2100).contains(&int(g)));
                let min_len = groups.iter().map(|g| g.chars().count()).min().unwrap_or(0);
                if !(c.get(1).is_some()
                    || groups
                        .iter()
                        .any(|g| g.chars().count() > 1 && g.starts_with('0'))
                    || (min_len >= 4 && !years)
                    || matches!(
                        identifier(text, c.get(0).unwrap().start()),
                        Some("strong") | Some("pin")
                    ))
                {
                    return None; // 10-15 and 2019-2021 are ranges
                }
            }
            return Some(Self::digit_groups(groups, "zero"));
        }
        if groups.len() == 2 && kinds[1] == 'd' && (kinds[0] == 'u' || kinds[0] == 'l') {
            let (letters, number) = (groups[0], groups[1]);
            if (kinds[0] == 'l' && letters.chars().count() > 1) || self.reads_as_word(letters) {
                return None; // pre-2020, COVID-19, SAVE-20
            }
            if short_number(number) {
                return Some(format!("{} {}", spelled(letters), cardinal(int(number))));
            }
            return Some(format!("{} , {}", spelled(letters), digits(number, "zero")));
        }
        if groups.len() == 2
            && kinds[0] == 'd'
            && (kinds[1] == 'l'
                || groups[1].chars().count() == 1
                || is_match(rx!(r"^\d+[A-Za-z]+$"), groups[1]))
        {
            return None; // 5-year, 3-D, 5-10MB
        }
        if kinds.iter().zip(groups).any(|(k, g)| {
            (*k == 'l' && g.chars().count() > 2)
                || (*k == 'm' && is_match(rx!(r"^\d+(?:s|st|nd|rd|th)$"), g))
        }) {
            return None; // 20-year-old, F-16s
        }
        if !token.chars().any(|ch| ch.is_uppercase())
            && kinds.iter().any(|k| *k == 'l' || *k == 'm')
        {
            return None; // 3-in-1, 50s-60s
        }
        let mut parts = Vec::new();
        for (kind, g) in kinds.iter().zip(groups) {
            parts.push(match kind {
                'd' => digits(g, "zero"),
                'm' => Self::alnum(g, "zero"),
                _ if g.chars().count() > 3 && self.is_word(&g.to_lowercase()) => g.to_string(),
                _ => {
                    if g.chars().count() <= 5 {
                        spelled(g)
                    } else {
                        g.to_string()
                    }
                }
            });
        }
        Some(parts.join(" , "))
    }

    /// Codes written with hyphens: AB-4471-Z, 91-220-0017, #A7-4419.
    fn code(&self, c: &Caps, text: &str) -> String {
        let groups: Vec<&str> = group(c, 2).split('-').collect();
        match self.code_words(&groups, c, text) {
            None => whole(c),
            Some(words) => format!(
                "{}{words} ",
                if c.get(1).is_some() { " number " } else { " " }
            ),
        }
    }

    /// Digits written in groups with spaces: a telephone number, a card or
    /// tracking number, or a large number with spaces between the thousands.
    fn grouped(&self, c: &Caps, text: &str) -> String {
        let m = c.get(0).unwrap();
        let plus = c.name("plus").map(|m| m.as_str());
        let mut lead = c.name("lead").map(|m| m.as_str());
        let mut trail = c.name("trail").map(|m| m.as_str());
        let core: Vec<&str> = named(c, "core").split(' ').collect();
        let (mut head, mut tail) = (String::new(), String::new());
        if lead.is_some_and(|l| capital_words().contains(l)) {
            head = format!("{} ", lead.unwrap());
            tail = trail.map(|t| format!(" {t}")).unwrap_or_default();
            lead = None;
            trail = None;
        }
        let kind = identifier(text, m.start());
        let pure = core.iter().all(|p| is_digits(p));
        if pure && plus.is_none() && lead.is_none() {
            let following = &text[m.end()..];
            let units = rx!(format!(r"(?i)^\s?(?:{})(?![A-Za-z])", self.n.unit_names));
            let amount = self.counted(text, m.end()) || is_match(units, following);
            if kind.is_none()
                && !core[0].starts_with('0')
                && core[0].chars().count() <= 3
                && core[1..].iter().all(|p| p.chars().count() == 3)
                && (core[core.len() - 1] == "000" || (amount && core.len() > 2))
            {
                return format!("{head}{}{tail}", core.join(","));
            }
            if !(kind.is_some()
                || (core[0].chars().count() > 1 && core[0].starts_with('0'))
                || (core.len() > 2 && core.iter().any(|p| p.chars().count() > 2)))
            {
                return whole(c);
            }
        } else if !pure && lead.is_none() && plus.is_none() && kind.is_none() {
            return whole(c);
        }
        let total: usize = core.iter().map(|p| p.chars().count()).sum();
        let zero = if plus.is_some()
            || kind == Some("phone")
            || (pure && core[0].starts_with('0') && total >= 10)
        {
            "oh"
        } else {
            "zero"
        };
        let mut spoken = if pure {
            Self::digit_groups(&core, zero)
        } else {
            core.iter()
                .map(|p| {
                    if is_digits(p) {
                        digits(p, zero)
                    } else {
                        Self::alnum(p, zero)
                    }
                })
                .collect::<Vec<_>>()
                .join(" , ")
        };
        if let Some(l) = lead {
            spoken = format!("{} , {spoken}", spelled(l));
        }
        if let Some(t) = trail {
            spoken = format!("{spoken} , {}", spelled(t));
        }
        format!(
            "{head}{}{spoken} {tail}",
            if plus.is_some() { " plus " } else { " " }
        )
    }

    /// Letters and digits written together: B12, 14C, X7K, PS5.
    fn glued(&self, c: &Caps) -> String {
        let mut out = Vec::new();
        for run in find_all(rx!(r"[A-Za-z]+|\d+"), &c[0]) {
            if is_digits(&run) {
                out.push(if short_number(&run) {
                    cardinal(int(&run))
                } else {
                    digits(&run, "zero")
                });
            } else if run.chars().count() == 1
                || ((is_upper(&run) || run.chars().count() <= 3) && !self.reads_as_word(&run))
            {
                out.push(spelled(&run));
            } else {
                out.push(run);
            }
        }
        format!(" {} ", out.join(" "))
    }

    // -- web addresses and file names --

    /// One run of letters inside an address: a word, or letters to name.
    fn address_part(&self, part: &str) -> String {
        let lower = part.to_lowercase();
        let n = part.chars().count();
        if n > 1 && self.is_word(&lower) && (n > 2 || short_words().contains(lower.as_str())) {
            return part.to_string();
        }
        if n <= 3 {
            spelled(part)
        } else {
            part.to_string()
        }
    }

    fn spoken_address(&self, core: &str) -> String {
        let pieces = find_all(rx!(r"[A-Za-z]+|\d+|."), core);
        let mail = core.contains('@');
        let mut host = false;
        let mut out: Vec<String> = Vec::new();
        for (index, piece) in pieces.iter().enumerate() {
            let following = pieces.get(index + 1).map(String::as_str).unwrap_or("");
            if piece == "@" {
                host = true;
            }
            let lower = piece.to_lowercase();
            if is_digits(piece) {
                out.push(address_digits(piece));
            } else if !piece.chars().all(is_ascii_alpha_char) {
                out.push(separator(piece).to_string());
            } else if index > 0
                && pieces[index - 1] == "."
                && (host || !mail)
                && (["", "/", ":", "?", "#"].contains(&following)
                    || (piece.chars().count() <= 3
                        && (self.n.endings.contains_key(&lower) || !self.is_word(&lower))))
            {
                // the ending of a file name or an address: a word if the table says so, otherwise letters
                out.push(if self.n.endings.get(&lower).is_some_and(|e| e == "word") {
                    lower
                } else {
                    spelled(piece)
                });
            } else {
                let mut letters = String::new();
                for part in find_all(rx!(r"[A-Z]?[a-z]+|[A-Z]+(?![a-z])"), piece) {
                    let spoken = self.address_part(&part);
                    if spoken.starts_with(SPELL) {
                        letters += &part;
                        continue;
                    }
                    if !letters.is_empty() {
                        out.push(spelled(&std::mem::take(&mut letters)));
                    }
                    out.push(spoken);
                }
                if !letters.is_empty() {
                    out.push(spelled(&letters));
                }
            }
        }
        out.iter()
            .filter(|s| !s.is_empty())
            .cloned()
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Web and mail addresses, file names and paths.
    fn address(&self, c: &Caps) -> String {
        let whole_text = c[0].to_string();
        let text = whole_text.trim_end_matches(['.', ',', ';', ':', '!', '?', ')']);
        let tail = &whole_text[text.len()..];
        if c.name("file").is_some() {
            let ending = named(c, "ending");
            let known = self.n.endings.contains_key(&ending.to_lowercase());
            let marked = c.name("folder").is_some()
                || c.name("path").is_some()
                || named(c, "stem").contains('_');
            if !(known
                || (marked
                    && ending.chars().count() <= 5
                    && (is_lower(ending) || is_upper(ending))
                    && !self.is_word(&ending.to_lowercase())))
            {
                return whole_text; // "the end.Then": a missing space, not a file
            }
        }
        let core = sub_str(rx!(r"^(?:https?://)?(?:www\.)?"), text, "");
        let core = core.trim_end_matches('/');
        format!(" {} {tail}", self.spoken_address(core))
    }

    /// Chat spellings that contain a digit or a slash: gr8, b4, 2day.
    fn chat_form(&self, c: &Caps) -> String {
        let Some(said) = self.n.chat.get(&c[0].to_lowercase()) else {
            return whole(c);
        };
        if said == "-" {
            return " ".into();
        }
        let parts: Vec<String> = said
            .split_whitespace()
            .map(|p| {
                if let Some(s) = p.strip_prefix('*') {
                    format!("{SPELL}{s}{END}")
                } else {
                    p.to_string()
                }
            })
            .collect();
        format!(" {} ", parts.join(" "))
    }

    /// `{...}`: a pronunciation written by the user.
    fn inline_phones(&mut self, c: &Caps) -> String {
        let symbols: Vec<String> = group(c, 2).split_whitespace().map(String::from).collect();
        if symbols.is_empty()
            || symbols
                .iter()
                .all(|s| s.chars().count() == 1 && s.is_ascii())
        {
            return whole(c);
        }
        let refs: Vec<&str> = symbols.iter().map(String::as_str).collect();
        let phones = if is_arpabet(&refs) {
            convert("", &symbols, None)
        } else {
            from_text(&symbols.join(" "))
        };
        let Ok(phones) = phones else { return whole(c) };
        self.inline.push((group(c, 1).to_string(), phones));
        let mark = char::from_u32(INLINE_BASE + self.inline.len() as u32 - 1).unwrap_or(' ');
        format!(" {INLINE}{mark}{INLINE} ")
    }

    /// `U.S.`, `e.g.`, `p.m.`: letters with full stops.
    fn dotted(&self, c: &Caps, text: &str) -> String {
        let m = c.get(0).unwrap();
        let letters = m.as_str().replace('.', "");
        let key = letters.to_lowercase();
        let following = text[m.end()..].trim_start();
        let words = match key.as_str() {
            "eg" => "for example".to_string(),
            "ie" => "that is".to_string(),
            _ => format!("{SPELL}{}{END}", letters.to_uppercase()),
        };
        let first = search(rx!(r"^[A-Za-z']+"), following).map(|c| c[0].to_lowercase());
        let ends = following.is_empty()
            || (first_char(following).is_uppercase()
                && first.is_some_and(|f| sentence_starters().contains(f.as_str())));
        format!(
            " {words}{} ",
            if ends && m.as_str().ends_with('.') {
                " ."
            } else {
                ""
            }
        )
    }

    fn abbreviation(&self, c: &Caps, text: &str) -> String {
        let m = c.get(0).unwrap();
        let (token, stop) = (group(c, 1), group(c, 2));
        let key = token.to_lowercase();
        let following = &text[m.end()..];
        let next_word = search(rx!(r"^\s*([A-Za-z]+|\d)"), following).map(|c| c[1].to_string());
        let capital_next = next_word
            .as_deref()
            .is_some_and(|w| first_char(w).is_uppercase());
        let capital_before = is_match(rx!(r"[A-Z][a-z]+\s+$"), &text[..m.start()]);
        if (key == "st" || key == "dr") && first_char(token).is_uppercase() {
            if capital_next && (!stop.is_empty() || key == "dr") {
                return if key == "st" { " saint " } else { " doctor " }.into();
            }
            if capital_before && !stop.is_empty() {
                return format!(
                    "{}{}",
                    if key == "st" { " street" } else { " drive" },
                    if following.trim().is_empty() {
                        " ."
                    } else {
                        " "
                    }
                );
            }
            return whole(c);
        }
        let Some((when, words)) = self.n.abbreviations.get(&key) else {
            return whole(c);
        };
        let number_next = next_word.as_deref().is_some_and(is_digits);
        if when == "title" {
            if !(first_char(token).is_uppercase() && (capital_next || !stop.is_empty())) {
                return whole(c);
            }
        } else if when == "number" {
            let previous =
                search(rx!(r"([A-Za-z]+)\.?\s+$"), &text[..m.start()]).map(|p| p[1].to_lowercase());
            if !(!stop.is_empty()
                && (number_next
                    || (key == "no" && previous.is_some_and(|p| numbered().contains(p.as_str())))))
            {
                return whole(c);
            }
        } else if stop.is_empty() || (when == "small" && is_upper(token)) {
            return whole(c);
        } else if ["mar", "sun", "sat", "jan", "dec", "max", "min", "est", "no"]
            .contains(&key.as_str())
            && !first_char(token).is_uppercase()
            && !number_next
        {
            // also words; "max. load" and "est. time" are the abbreviation, "he sat." is not
            if !(["max", "min", "est"].contains(&key.as_str())
                && next_word
                    .as_deref()
                    .is_some_and(|w| first_char(w).is_lowercase()))
            {
                return whole(c);
            }
        }
        let words = if key == "est" && !number_next {
            "estimated"
        } else {
            words.as_str()
        };
        let ends = when != "title"
            && (following.trim().is_empty()
                || (capital_next
                    && next_word
                        .as_deref()
                        .is_some_and(|w| sentence_starters().contains(w.to_lowercase().as_str()))));
        format!(" {words}{} ", if ends { " ." } else { "" })
    }

    /// The text with every non-word written out, markers included.
    fn rewrite(&mut self, text: &str) -> String {
        use unicode_normalization::UnicodeNormalization;
        let mut text = sub(
            rx!(r"(?<![\d.,/])(?:(\d+)\s?)?([½⅓⅔¼¾⅕⅖⅗⅘⅙⅚⅐⅛⅜⅝⅞⅑⅒])"),
            text,
            |c| self.vulgar(c),
        );
        text = sub(&self.n.area, &text, |c| self.square(c));
        text = text.replace('²', " squared ").replace('³', " cubed ");
        text = text
            .nfkc()
            .collect::<String>()
            .replace('⁄', "/")
            .replace('−', " minus ");
        if !text.is_ascii() {
            let accented = rx!(format!(
                "(?i)\\b({})(s?)\\b",
                ACCENT_DECIDES
                    .iter()
                    .map(|(w, _)| *w)
                    .collect::<Vec<_>>()
                    .join("|")
            ));
            text = sub(accented, &text, |c| {
                let phones = ACCENT_DECIDES
                    .iter()
                    .find(|(w, _)| *w == group(c, 1).to_lowercase())
                    .map(|(_, p)| *p)
                    .unwrap_or("");
                format!(
                    "{{{phones}{}}}",
                    if group(c, 2).is_empty() { "" } else { " Z" }
                )
            });
            text = text.chars().map(plain_letter).collect();
        }
        text = sub_str(rx!(r"(?<=\d)–(?=\d)"), &text, "-");
        text = sub(
            rx!(format!(
                r"\b({DAY_PATTERN}|{MONTH_PATTERN})\b(\.?)\s?[-–]\s?({DAY_PATTERN}|{MONTH_PATTERN})\b(\.?)"
            )),
            &text.clone(),
            |c| self.day_range(c, &text),
        );
        text = replace_all(
            &text,
            &[
                ("‘", "'"),
                ("’", "'"),
                ("‚", "'"),
                ("‛", "'"),
                ("`", "'"),
                ("´", "'"),
                ("“", "\""),
                ("”", "\""),
                ("„", "\""),
                ("‟", "\""),
                ("«", "\""),
                ("»", "\""),
                ("–", "—"),
                ("―", "—"),
                ("[", "("),
                ("]", ")"),
            ],
        );
        self.inline.clear();
        text = sub(rx!(r"([A-Za-z']+)?\{([^{}]*)\}"), &text, |c| {
            self.inline_phones(c)
        });
        text = text.replace('{', "(").replace('}', ")");
        let address = rx!(
            r#"(?P<url>(?:https?://|www\.)[^\s<>"']+)|(?P<mail>[\w.+-]+@[\w-]+(?:\.[\w-]+)+)|(?P<file>(?<![\w@./-])(?P<folder>/?(?:[\w~-]+/)+)?(?P<stem>[\w-]+(?:\.[\w-]+)*)\.(?P<ending>[A-Za-z][A-Za-z0-9]{0,9})(?![\w-])(?P<path>/[^\s<>"']*)?)"#
        );
        text = sub(address, &text, |c| self.address(c));
        text = text.replace('→', " to ");
        text = sub_str(rx!(EMOJI), &text, " ");
        let emoticon = rx!(
            r#"(?<![A-Za-z0-9])(?:[:;=]['^-]?[DPpOo3|/\\*\]\[}{><]+|[)(\]\[][-^']?[:;=]|<3+|</3|\^_?\^|-_-|[oO]_[oO]|>_<|T_T|\\o/|:'[()])(?![A-Za-z0-9])|[:;]['^-]?[)(]+|(?<![A-Za-z0-9)])=['^-]?[)(]+|=['^-]?[)(]+(?![A-Za-z0-9(.$-])|(?<=[A-Za-z]{2})<3+(?![.,]?\d)"#
        );
        text = sub_str(emoticon, &text, " ");
        text = sub_str(rx!(r"\.{3,}"), &text, "…");
        text = sub_str(rx!(r"(?<=\d)\s-\s(?=\d[\d.,]*\s?=)"), &text, " minus ");
        text = sub(
            rx!(r"(?<![\d.,/-])(\d[\d.,]*)-(?=\d[\d.,]*=)"),
            &text,
            |c| format!("{} minus ", group(c, 1)),
        );
        text = sub_str(rx!(r"-{2,}|\s-\s"), &text, "—");
        text = sub_str(rx!(r"(?i)\bw/o\b"), &text, " without ");
        text = sub_str(rx!(r"(?i)\bw/(?=\s|$)"), &text, " with ");
        text = sub_str(rx!(r"(?i)\bn/a\b"), &text, " not applicable ");
        text = text.replace("'n'", " 'n ");
        if is_match(rx!(r"[<>≥≤≠]|!="), &text) {
            text = self.mark_comparisons(&text);
        }
        if text.chars().any(|c| c.is_numeric()) && is_match(rx!(r"\d"), &text) {
            text = self.figures(&text);
        }
        self.signs(&text)
    }

    /// The passes that need a digit: money, times, dates, telephone numbers,
    /// versions, codes, units, fractions, ranges.
    fn figures(&mut self, text: &str) -> String {
        let sign_after = format!(r"[$£€¥₹](?![\w$£€¥₹])|(?:{CURRENCY_CODES})(?![A-Za-z])");
        let mut text = sub(
            rx!(
                r"(?<![\w$])(?<!\d\s)\$(?:(?=[^\s\d$.,;:!?)])[^$\n]{0,80}|(?=\d)[^$\n]{0,80}[=^_\\{}+*/<>][^$\n]{0,80}|(?=\s)[^$\n]{0,80}[=^_\\{}+*<>][^$\n]{0,80})\d\s?\$(?![\w$])"
            ),
            text,
            |c| {
                let m = &c[0];
                format!("{}{FORMULA_SIGN}", &m[..m.len() - 1])
            },
        );
        text = sub(
            rx!(r"(?<![\w.,])([1-9]\d{0,2}(?:\.\d{3})+),(\d{1,2})(?!\d|[.,]\d)"),
            &text,
            Self::continental,
        );
        text = sub(
            rx!(format!(
                r"((?:[$£€¥₹]|(?<![A-Za-z])(?:{CURRENCY_CODES}))\s?-?\d+),(\d{{1,2}})(?!\d|[.,]\d)"
            )),
            &text,
            |c| format!("{}.{}", group(c, 1), group(c, 2)),
        );
        text = sub(
            rx!(format!(
                r"(?<![\w.,])(\d+),(\d{{1,2}})(?=\s?(?:%|{sign_after}))"
            )),
            &text,
            |c| format!("{}.{}", group(c, 1), group(c, 2)),
        );
        let money = rx!(format!(
            r"(?:(?<![\w.])(?P<minus>-)\s?)?(?P<symbol>[$£€¥₹]|(?<![A-Za-z])(?:{CURRENCY_CODES})(?![A-Za-z]))\s?(?P<inner>-)?{AMOUNT}(?:\s?(?P<scale>thousand|million|billion|trillion|bn|[kKmMbB])(?![A-Za-z]))?"
        ));
        text = sub(money, &text, |c| self.money(c));
        text = sub(
            rx!(format!(
                r"(?<![\w.,-])(\d[\d,]*(?:\.\d+)?)-(?=\d[\d,]*(?:\.\d+)?\s?(?:{sign_after}))"
            )),
            &text,
            |c| format!("{} to ", group(c, 1)),
        );
        let money_after = rx!(format!(
            r"(?:(?<![\w.])(?P<minus>-)\s?)?(?<![\w.,^])(?P<whole>\d{{1,3}}(?:,\d{{3}})+|[1-9]\d{{0,2}}(?: \d{{3}})+|\d+)(?:\.(?P<fraction>\d+))?\s?(?P<symbol>{sign_after})"
        ));
        text = sub(money_after, &text, |c| self.money(c));
        text = sub(rx!(r"(?<![\w.])(\d[\d,]*(?:\.\d+)?)\s?¢"), &text, |c| {
            format!(
                " {} {} ",
                number_words(group(c, 1)),
                if group(c, 1) == "1" { "cent" } else { "cents" }
            )
        });

        // times of day, lengths of time, ratios
        let clock = r"\d{1,2}:[0-5]\d(?:\s?[AaPp]\.?[Mm]\.?)?|\b\d{1,2}\s?[AaPp]\.?[Mm]\.?";
        text = sub(rx!(format!(r"({clock})\s({ZONES})\b")), &text, |c| {
            format!("{} {}", group(c, 1), spelled(group(c, 2)))
        });
        text = sub(
            rx!(r"(?<![\d.,:])(\d{1,2})\.([0-5]\d)(?=\s?[AaPp]\.?[Mm]\b)"),
            &text,
            |c| format!("{}:{}", group(c, 1), group(c, 2)),
        );
        let snapshot = text.clone();
        text = sub(
            rx!(
                r"(?<![\d.,:])(\d{1,2})\.(00|15|30|45)(\s?(?:to|until|till|and|[-—])\s?)(\d{1,2})\.(00|15|30|45)(?!\d|\.\d)"
            ),
            &snapshot,
            |c| self.dot_times(c, &snapshot),
        );
        text = sub(
            rx!(format!(
                r"({clock})\s?[-—]\s?(?=\d{{1,2}}(?::[0-5]\d|\s?[AaPp]\.?[Mm]\b))"
            )),
            &text,
            |c| format!("{} to ", group(c, 1)),
        );
        let snapshot = text.clone();
        text = sub(
            rx!(r"(?<![\d:.,])(\d{1,3}):(\d{1,3}(?:,\d{3})+|\d+)(?![\d:]|[.,]\d)"),
            &snapshot,
            |c| self.ratio(c, &snapshot),
        );
        let snapshot = text.clone();
        text = sub(
            rx!(format!(
                r"(?<![\d:])(\d{{1,2}}):([0-5]\d)(?::([0-5]\d))?{HALF_DAY}?(?![\d:])"
            )),
            &snapshot,
            |c| self.time(c, &snapshot),
        );
        let snapshot = text.clone();
        text = sub(
            rx!(format!(r"(?<![\d.:,])(\d{{1,2}}){HALF_DAY}(?![A-Za-z])")),
            &snapshot,
            |c| {
                let m = c.get(0).unwrap();
                format!(
                    " {} {}",
                    cardinal(int(group(c, 1))),
                    Self::half_day(&snapshot, m.end(), m.as_str(), group(c, 2), group(c, 3))
                )
            },
        );

        // dates
        let slash = rx!(r"(?<![\d/.])(\d{1,2})/(\d{1,2})(?:/(?:\d{4}|\d{2}))?(?![\d/])");
        let seen: Vec<(i128, i128)> = slash
            .captures_iter(&text)
            .filter_map(|c| c.ok())
            .map(|c| (int(group(&c, 1)), int(group(&c, 2))))
            .filter(|p| *p != (24, 7))
            .collect();
        self.day_first = seen.iter().any(|(a, b)| *a > 12 && 12 >= *b)
            && !seen.iter().any(|(a, b)| *b > 12 && 12 >= *a);
        self.dated = is_match(
            rx!(r"(?<![\d/])\d{1,2}/\d{1,2}/(?:\d{4}|\d{2})(?![\d/])"),
            &text,
        );
        text = sub(
            rx!(r"(?<![\d-])(\d{4})-(\d{1,2})-(\d{1,2})(?![\d-])"),
            &text,
            |c| {
                Self::date(int(group(c, 2)), int(group(c, 3)), group(c, 1), false)
                    .unwrap_or_else(|| whole(c))
            },
        );
        text = sub(
            rx!(r"(?<![\d/])(\d{4})/(\d{1,2})/(\d{1,2})(?![\d/])"),
            &text,
            |c| {
                Self::date(int(group(c, 2)), int(group(c, 3)), group(c, 1), false)
                    .unwrap_or_else(|| whole(c))
            },
        );
        text = sub(
            rx!(r"(?<![\d.])(\d{1,2})\.(\d{1,2})\.(\d{4})(?!\d|\.\d)"),
            &text,
            |c| self.dot_date(c),
        );
        text = sub(
            rx!(r"(?<![\d/])(\d{1,2})/(\d{1,2})/(\d{4}|\d{2})(?![\d/])"),
            &text,
            |c| self.slash_date(c),
        );
        text = sub(
            rx!(format!(
                r"\b({MONTH_PATTERN})\.?\s+(\d{{1,2}})(?:st|nd|rd|th)?\s?[-—]\s?(\d{{1,2}})(?:st|nd|rd|th)?(?:,?\s+(\d{{4}}))?(?![\d:])"
            )),
            &text,
            |c| self.days_of_month(c),
        );
        text = sub(
            rx!(format!(
                r"(?<![\d.,/:-])(\d{{1,2}})(?:st|nd|rd|th)?\s?[-—]\s?(\d{{1,2}})(?:st|nd|rd|th)?\s+({MONTH_PATTERN})\b(?:,?\s+(\d{{4}}))?"
            )),
            &text,
            |c| self.days_of_month(c),
        );
        text = sub(
            rx!(format!(
                r"\b({MONTH_PATTERN})\.?\s+(\d{{1,2}})(?:st|nd|rd|th)?(?:,?\s+(\d{{4}}))?(?![\d:])"
            )),
            &text,
            |c| {
                Self::date(
                    month_number(group(c, 1)) as i128,
                    int(group(c, 2)),
                    group(c, 3),
                    false,
                )
                .unwrap_or_else(|| whole(c))
            },
        );
        text = sub(
            rx!(format!(
                r"(?<![\d.,/:-])(\d{{1,2}})(?:st|nd|rd|th)?\s+({MONTH_PATTERN})\b(\.(?=\s+[a-z\d]))?(?:,?\s+(\d{{4}}))?"
            )),
            &text,
            |c| self.day_month(c),
        );

        // telephone numbers, versions, codes
        text = sub(
            rx!(r"(?<![\d-])(?:\+?(1)[-.\s])?\(?(\d{3})\)?[-.\s](\d{3})[-.\s](\d{4})(?![\d-])"),
            &text,
            |c| self.phone(c),
        );
        text = sub(rx!(r"(?<![\w.,-])(\d{3})-(\d{4})(?![\w-])"), &text, |c| {
            self.phone(c)
        });
        text = sub(
            rx!(r"(?<![\w.])([vV])?(\d+(?:\.\d+){2,})(?:-(\d+)(?![\w-]|\.\d))?(?![\w])"),
            &text,
            |c| {
                let v = c
                    .get(1)
                    .map(|m| format!("{} ", spelled(m.as_str())))
                    .unwrap_or_default();
                let parts = group(c, 2)
                    .split('.')
                    .map(version_part)
                    .collect::<Vec<_>>()
                    .join(" point ");
                let dash = c
                    .get(3)
                    .map(|m| format!(" dash {}", number_words(m.as_str())))
                    .unwrap_or_default();
                format!(" {v}{parts}{dash} ")
            },
        );
        text = sub(
            rx!(r"(?<![\w.])([vV])(\d+)\.(\d{2,3})(?!\d|\.\d|\w)"),
            &text,
            |c| {
                format!(
                    " {} {} point {} ",
                    spelled(group(c, 1)),
                    cardinal(int(group(c, 2))),
                    minor(group(c, 3))
                )
            },
        );
        text = sub(
            rx!(format!(
                r"(?i)\b({VERSIONED})(\.?\s(?:(?:from|to)\s)?)(\d+)\.(\d{{2,3}})(?:(\s(?:to|or|and)\s)(\d+)\.(\d{{2,3}}))?(?!\d|\.\d|\s?%|\s(?:percent|million|billion)\b)"
            )),
            &text,
            |c| {
                let second = c
                    .get(5)
                    .map(|_| {
                        format!(
                            "{}{} point {}",
                            group(c, 5),
                            cardinal(int(group(c, 6))),
                            minor(group(c, 7))
                        )
                    })
                    .unwrap_or_default();
                format!(
                    "{}{}{} point {}{second} ",
                    group(c, 1),
                    group(c, 2),
                    cardinal(int(group(c, 3))),
                    minor(group(c, 4))
                )
            },
        );
        let snapshot = text.clone();
        text = sub(
            rx!(r"(?<![\w.,/-])(#)?([A-Za-z0-9]+(?:-[A-Za-z0-9]+)+)(?![\w-]|[.,]\d)"),
            &snapshot,
            |c| self.code(c, &snapshot),
        );
        text = sub(
            rx!(r"(?<![\w.,/-])(\d+(?:/\d+){2,})(?![\w/]|[.,]\d)"),
            &text,
            |c| {
                format!(
                    " {} ",
                    Self::digit_groups(&group(c, 1).split('/').collect::<Vec<_>>(), "zero")
                )
            },
        );
        let snapshot = text.clone();
        let grouped = rx!(
            r"(?<![\w.,:/#+-])(?P<plus>\+)?(?:(?P<lead>[A-Z]{2}) )?(?P<core>[A-Z0-9]*\d[A-Z0-9]*(?: [A-Z0-9]*\d[A-Z0-9]*)+)(?(lead)(?: (?P<trail>[A-Z]{2})(?![\w]))?)(?![\w]|[.,:/-]\d|\s?[%°])"
        );
        text = sub(grouped, &snapshot, |c| self.grouped(c, &snapshot));

        text = sub_str(rx!(r"(?<![\w.])-(?=\d)"), &text, " minus ");
        text = sub_str(
            rx!(format!(
                r"(?:(?<=\d)\s?[xX×]|×)\s?(?=\d[\d.,]*\s?(?:{LENGTHS})\b)"
            )),
            &text,
            " by ",
        );
        text = sub_str(rx!(r"(?<=\d)\s[xX×]\s(?=\d)"), &text, " times ");
        text = sub(
            rx!(r"(?<![\w/'])[A-Za-z0-9/]*[0-9/][A-Za-z0-9/]*(?![\w/'])"),
            &text,
            |c| self.chat_form(c),
        );
        text = sub(rx!(r"(?<![\w.])'?(\d{2}|\d{4})'?s\b"), &text, |c| {
            let n = int(group(c, 1));
            format!(
                " {} ",
                plural_of(&if group(c, 1).chars().count() == 4 {
                    year(n)
                } else {
                    cardinal(n)
                })
            )
        });
        text = sub(rx!(r"(?<![\w.])(\d+)(?:st|nd|rd|th)\b"), &text, |c| {
            format!(" {} ", ordinal(int(group(c, 1))))
        });
        text = sub(
            rx!(r"(?<![\w.])(144|240|360|480|540|576|720|900|1080|1440|2160|4320)([pi])\b"),
            &text,
            |c| format!(" {} {} ", pairs(int(group(c, 1))), spelled(group(c, 2))),
        );
        text = sub(rx!(r"(\d[\d,]*(?:\.\d+)?)\s?%"), &text, |c| {
            format!(" {} percent ", number_words(group(c, 1)))
        });
        text = sub(&self.n.area_plain, &text, |c| self.square(c));
        let snapshot = text.clone();
        text = sub(&self.n.unit, &snapshot, |c| self.measure(c, &snapshot));
        text = sub(
            rx!(r"(?<![\d/.,-])(\d+) (\d)/(2|3|4|5|6|8|10|16)(?![\d/]|\.\d)"),
            &text,
            |c| self.mixed_fraction(c),
        );
        let snapshot = text.clone();
        text = sub(
            rx!(r"(?<![\d/.])(\d+)/(\d+)(?![\d/]|\.\d)"),
            &snapshot,
            |c| self.fraction(c, &snapshot),
        );
        text = sub(
            rx!(r"(?<![\w.,-])(\d{4})[-–](\d{4})(?![\w.,-])"),
            &text,
            |c| format!(" {} to {} ", year(int(group(c, 1))), year(int(group(c, 2)))),
        );
        text = sub(
            rx!(r"(?<![\w.,-])(\d+)[-–](\d+)(?![\w-]|[.,]\d)"),
            &text,
            |c| {
                format!(
                    " {} to {} ",
                    number_words(group(c, 1)),
                    number_words(group(c, 2))
                )
            },
        );
        text = sub(rx!(r"(?<![\w.])(\d+)x\b"), &text, |c| {
            format!(" {} times ", number_words(group(c, 1)))
        });
        sub_str(rx!(r"(?<=\d)\^(?=\d)"), &text, " to the power of ")
    }

    /// Mark the letters that stand for something, to be named: "a=10", "b>a", "(k)".
    fn named_letters(text: &str) -> String {
        if !text.chars().any(|c| "=<>+*/^()≤≥≠≈×÷±".contains(c)) {
            return text.to_string();
        }
        let plain = without_angles(text);
        let places: HashSet<usize> = letter_places()
            .iter()
            .flat_map(|p| {
                p.captures_iter(&plain)
                    .filter_map(|c| c.ok())
                    .filter_map(|c| c.name("letter").map(|m| m.start()))
                    .collect::<Vec<_>>()
            })
            .collect();
        if places.is_empty() {
            return text.to_string();
        }
        text.char_indices()
            .map(|(i, c)| {
                if places.contains(&i) {
                    format!("{SPELL}{}{END}", c.to_uppercase())
                } else {
                    c.to_string()
                }
            })
            .collect()
    }

    /// Put the mark for "is" after the first value of each comparison between
    /// two values: "67 > 66", "b>a", "f(x) >= 0".
    fn mark_comparisons(&self, text: &str) -> String {
        let plain = without_angles(text);
        sub(compared(), text, |c| {
            let m = c.get(0).unwrap();
            let (before, after) = (&plain[..m.start()], &plain[m.end()..]);
            let sign = c.name("sign").unwrap();
            let value = search(value_before(), before);
            if plain[sign.start()..].chars().next() != sign.as_str().chars().next()
                || value.is_none()
                || !is_match(value_after(), after)
            {
                return whole(c);
            }
            let value = value.unwrap();
            let v0 = value.get(0).unwrap();
            let chars: Vec<char> = before.chars().collect();
            let mut start = before[..v0.start()].chars().count() as isize;
            if v0.as_str() == ")" || v0.as_str() == "]" {
                let mut depth = 0isize;
                while start >= 0 {
                    let ch = chars[start as usize];
                    depth += (ch == ')' || ch == ']') as isize - (ch == '(' || ch == '[') as isize;
                    if depth == 0 {
                        break;
                    }
                    start -= 1;
                }
                while start > 0
                    && (chars[start as usize - 1].is_alphanumeric()
                        || chars[start as usize - 1] == '_'
                        || chars[start as usize - 1] == '.')
                {
                    start -= 1;
                }
            }
            let head: String = chars[..start.max(0) as usize].iter().collect();
            let word = search(rx!(r"[A-Za-z']+$"), head.trim_end()).map(|w| w[0].to_lowercase());
            if word.is_some_and(|w| !clause_openers().contains(w.as_str())) {
                return whole(c);
            }
            format!("{IS}{}", &c[0])
        })
    }

    /// Words for a comparison sign, after "is" where the mark for it stands before the sign.
    fn compared_words(c: &Caps, text: &str, words: &str) -> String {
        let m = c.get(0).unwrap();
        let marked =
            m.as_str().starts_with(IS) || text[..m.start()].trim_end_matches(' ').ends_with(IS);
        format!(" {}{words} ", if marked { "is " } else { "" })
    }

    /// "b>a", "x<5", "p<.05": the words for "<" and ">" written with no spaces.
    fn joined_comparisons(&self, text: &str) -> String {
        let plain = without_angles(text);
        sub(joined_comparison(), text, |c| {
            let m = c.get(0).unwrap();
            let before = text[..m.start()].trim_end_matches(IS);
            let after = &text[m.end()..];
            if plain[m.start()..].chars().next() != m.as_str().chars().next() {
                return whole(c); // the bracket of a tag
            }
            let figure_before = rx!(r"(?<![\w.,])\d[\d.,]*%?$");
            let figure_after = rx!(r"\A(?:\d[\d.,]*%?(?!\w)|\.\d| minus \d)");
            let letter_before = rx!(format!(r"(?:(?<![\w'’.])[A-Za-z]|[){END}])$"));
            let letter_after = rx!(format!(r"\A(?:[A-Za-z](?![\w'’])|[({SPELL}])"));
            if !(is_match(figure_before, before)
                || is_match(figure_after, after)
                || (is_match(letter_before, before) && is_match(letter_after, after)))
            {
                return whole(c);
            }
            Self::compared_words(
                c,
                text,
                if m.as_str() == "<" {
                    "less than"
                } else {
                    "greater than"
                },
            )
        })
    }

    /// The passes that follow: signs, abbreviations, and the numbers that are left.
    fn signs(&self, text: &str) -> String {
        let mut text = Self::named_letters(text);
        if text.contains('<') || text.contains('>') {
            text = self.joined_comparisons(&text);
        }
        text = sub(
            rx!(format!(r"(?<![\w-])({SIZES})-({SIZES})(?![\w-])")),
            &text,
            |c| format!(" {} to {} ", spelled(group(c, 1)), spelled(group(c, 2))),
        );
        text = sub_str(rx!(r"#(?=\d)"), &text, " number ");
        text = sub_str(rx!(r"#(?=[A-Z]{1,4}-?\d)"), &text, " number ");
        text = sub_str(rx!(r"#(?=[A-Za-z])"), &text, " hashtag ");
        text = sub_str(rx!(r"~(?=\s?\d)"), &text, " about ");
        let snapshot = text.clone();
        text = sub(
            rx!(format!(r"(?<=[\w\s{END}{IS}])(>=|<=|!=)(?=[\w\s{SPELL}])")),
            &snapshot,
            |c| Self::compared_words(c, &snapshot, comparison_word(group(c, 1))),
        );
        text = sub_str(rx!(r"(?<=\d) Ti\b"), &text, &format!(" {}", spelled("TI")));
        text = sub(rx!(r"\b([Rr])ef\.\s?[Nn]o\.(?=\s)"), &text, |c| {
            format!("{}eference number", group(c, 1))
        });
        if text.contains('.') {
            let snapshot = text.clone();
            text = sub(rx!(r"\b((?:[A-Za-z]\.){2,})"), &snapshot, |c| {
                self.dotted(c, &snapshot)
            });
        }
        let snapshot = text.clone();
        text = sub(
            rx!(r"\b([A-Za-z]{1,6})\b(\.?)(?![A-Za-z'’])"),
            &snapshot,
            |c| self.abbreviation(c, &snapshot),
        );
        if is_match(rx!(r"\d"), &text) {
            text = sub(
                rx!(
                    r"(?<![\w'.])(?=[A-Za-z0-9]*[A-Za-z])(?=[A-Za-z0-9]*\d)[A-Za-z0-9]+(?![\w']|\.\d)"
                ),
                &text,
                |c| self.glued(c),
            );
            let snapshot = text.clone();
            text = sub(
                rx!(r"(?<![\w.])\d{1,3}(?:,\d{3})+(?:\.\d+)?|\d+(?:\.\d+)?|(?<![\w.])\.\d+"),
                &snapshot,
                |c| self.number(c, &snapshot),
            );
        }
        text = sub_str(rx!(r"(?<=[A-Za-z])/(?=[A-Za-z])"), &text, " ");
        let snapshot = text.clone();
        text = sub(
            rx!(format!(r"(?<=\d){IS}?\s?[<>]\s?(?=\d)|(?<=\s)[<>](?=\s)")),
            &snapshot,
            |c| {
                Self::compared_words(
                    c,
                    &snapshot,
                    if c[0].contains('<') {
                        "less than"
                    } else {
                        "greater than"
                    },
                )
            },
        );
        let snapshot = text;
        let mut out = String::with_capacity(snapshot.len());
        for (i, ch) in snapshot.char_indices() {
            match symbol_word(ch) {
                Some(w) if "≥≤≠".contains(ch) => {
                    let marked = snapshot[..i].trim_end_matches(' ').ends_with(IS);
                    out.push_str(&format!(" {}{w} ", if marked { "is " } else { "" }));
                }
                Some(w) => out.push_str(&format!(" {w} ")),
                None => out.push(ch),
            }
        }
        out.replace(IS, "")
    }

    // -- words --

    /// Spellings a word stretched for emphasis may stand for.
    fn unstretched(word: &str) -> Vec<String> {
        if !is_match(rx!(r"(.)\1\1"), word) {
            return Vec::new();
        }
        let cut = |double_inside: bool, double_final: bool| -> String {
            sub(rx!(r"(.)\1{2,}"), word, |c| {
                let m = c.get(0).unwrap();
                let finalpos = m.end() == word.len();
                group(c, 1).repeat(
                    if finalpos {
                        double_final
                    } else {
                        double_inside
                    } as usize
                        + 1,
                )
            })
        };
        let all_m = word.chars().all(|c| c == 'm') && !word.is_empty();
        let trimmed = sub(rx!(r"(.)\1{2,}$"), word, |c| group(c, 1).to_string());
        let liked = is_match(rx!(r"([lfsz])\1\1+$"), word) || all_m || trimmed == "to";
        let mut ordered: Vec<String> = Vec::new();
        for candidate in [
            cut(true, liked),
            cut(true, !liked),
            cut(false, liked),
            cut(false, !liked),
        ] {
            if !ordered.contains(&candidate) {
                ordered.push(candidate);
            }
        }
        ordered
    }

    /// Tokens for one row of the chat table: words, letters to name, or nothing.
    fn chat_words(chat: &str, raw: &str) -> Vec<Word> {
        if chat == "-" {
            return Vec::new();
        }
        chat.split_whitespace()
            .map(|part| match part.strip_prefix('*') {
                Some(rest) => Word::spelled(&rest.to_lowercase(), raw),
                None => Word::new(&part.to_lowercase(), part),
            })
            .collect()
    }

    /// True when the word at `place` is a clause by itself and can be a reply.
    fn reply(out: &[Token], place: usize) -> bool {
        let mut marks: Vec<String> = Vec::new();
        for step in [-1isize, 1] {
            let mut index = place as isize + step;
            while index >= 0
                && (index as usize) < out.len()
                && matches!(&out[index as usize], Token::Punct(m) if "\"'()".contains(m.as_str()))
            {
                index += step;
            }
            if index < 0 || index as usize >= out.len() {
                marks.push(String::new());
            } else if matches!(out[index as usize], Token::Word(_)) {
                return false;
            } else if let Token::Punct(mark) = &out[index as usize] {
                marks.push(mark.clone());
                let beyond = index + step;
                if mark == "," && beyond >= 0 && (beyond as usize) < out.len() {
                    if let Token::Word(w) = &out[beyond as usize] {
                        if w.text.chars().count() == 1 && w.text != "i" {
                            return false;
                        }
                    }
                }
            }
        }
        marks != [",".to_string(), ",".to_string()]
    }

    fn word_tokens(
        &self,
        raw: &str,
        all_caps: bool,
        previous: &str,
        following: bool,
        fresh: bool,
    ) -> Vec<Word> {
        let lower = raw.to_lowercase();
        if let Some(chat) = self.n.chat.get(&lower) {
            if (raw.chars().count() > 1 || is_lower(raw)) && !self.n.replies.contains(&lower) {
                return Self::chat_words(chat, raw);
            }
        }
        if let Some(number) = roman(raw) {
            if !previous.is_empty() && first_char(previous).is_uppercase() {
                let words = if cardinal_after().contains(previous.to_lowercase().as_str()) {
                    cardinal(number)
                } else {
                    format!("the {}", ordinal(number))
                };
                return words.split(' ').map(|w| Word::new(w, raw)).collect();
            }
        }
        let letters = raw.replace('\'', "");
        let possessive = lower.ends_with("'s");
        let stem = if possessive {
            &raw[..raw.len() - 2]
        } else {
            raw
        };
        if stem.chars().count() == 1 && is_alpha(stem) {
            // "A" is the article unless it labels something: "Plan A.", "vitamin A"
            let label = stem == "A"
                && !previous.is_empty()
                && (!following
                    || (!fresh && labelled().contains(previous.to_lowercase().as_str())));
            if ["a", "A", "I", "i"].contains(&stem) && !possessive && !label {
                return vec![Word::new(&stem.to_lowercase(), raw)];
            }
            return vec![Word::spelled(&lower, raw)];
        }
        let hiss = stem.chars().count() > 2
            && "SsZz".contains(first_char(stem))
            && lower.chars().collect::<HashSet<_>>().len() == 1
            && (all_caps || !is_upper(stem));
        if hiss {
            return vec![Word {
                phones: Some(vec![(first_char(&lower).to_string(), 0)]),
                ..Word::new(&lower, raw)
            }];
        }
        if is_upper(stem)
            && is_alpha(stem)
            && (2..=5).contains(&stem.chars().count())
            && (!all_caps || !self.is_word(&stem.to_lowercase()))
            && (initialisms().contains(stem) || !self.is_word(&stem.to_lowercase()))
        {
            return vec![Word::spelled(&lower, raw)];
        }
        if !is_alpha(&letters) {
            return Vec::new();
        }
        // iOS, macOS, eSIM: small letters, then capitals that are named.
        let capitals = is_match(rx!(r"^[a-z]+(?:[A-Z][a-z]+)*[A-Z]{2,}(?:'s)?$"), raw);
        if capitals || !self.is_word(&lower) {
            if !capitals {
                for candidate in Self::unstretched(&lower) {
                    if self.is_word(&candidate) {
                        return vec![Word::new(&candidate, raw)];
                    }
                }
            }
            let parts = find_all(rx!(r"[A-Z]?[a-z']+|[A-Z]+(?![a-z])"), raw);
            if parts.len() > 1 && parts.concat() == raw {
                let mut out: Vec<Word> = Vec::new();
                for part in parts {
                    let named = if capitals && is_upper(&part) {
                        initialisms().contains(part.as_str())
                    } else {
                        false
                    };
                    let spelled = part.chars().count() == 1
                        || named
                        || (is_upper(&part) && !self.is_word(&part.to_lowercase()));
                    if capitals && out.last().is_some_and(|l| l.spell) && (spelled || part == "'s")
                    {
                        let last = out.last_mut().unwrap();
                        last.text.push_str(&part.to_lowercase());
                        last.raw.push_str(&part);
                    } else {
                        let attach = !out.is_empty();
                        out.push(Word {
                            attach,
                            spell: spelled,
                            ..Word::new(&part.to_lowercase(), &part)
                        });
                    }
                }
                return out;
            }
        }
        vec![Word::new(&lower, raw)]
    }

    fn tokens(mut self, text: &str) -> Vec<Token> {
        let text = self.rewrite(text);
        let runs = find_all(rx!(r"[A-Za-z]{2,}"), &text);
        let all_caps = runs.len() >= 3
            && runs.iter().filter(|r| is_upper(r)).count() as f64 >= 0.6 * runs.len() as f64;
        let pattern = rx!(format!("{SPELL}([A-Za-z ]+){END}|{INLINE}(.){INLINE}|('?[A-Za-z]+(?:'[A-Za-z]+)*)|(\\d+)|(-)|(\\S)"));
        let mut out: Vec<Token> = Vec::new();
        let (mut attach, mut previous, mut fresh) = (false, String::new(), true);
        let mut short: Vec<usize> = Vec::new();
        let mut replies: Vec<usize> = Vec::new();
        for c in pattern.captures_iter(&text) {
            let Ok(c) = c else { break };
            let m = c.get(0).unwrap();
            let before = text[..m.start()].chars().next_back().unwrap_or(' ');
            let next_char = text[m.end()..].chars().next();
            let rest = &text[m.end()..];
            if c.get(5).is_some() {
                attach = matches!(out.last(), Some(Token::Word(_)))
                    && !before.is_whitespace()
                    && next_char.is_some_and(|n| n.is_alphanumeric());
                continue;
            }
            let mut new: Vec<Word> = Vec::new();
            if let Some(s) = c.get(1) {
                new = vec![Word::spelled(
                    &s.as_str().replace(' ', "").to_lowercase(),
                    s.as_str(),
                )];
            } else if let Some(i) = c.get(2) {
                let n = (first_char(i.as_str()) as u32).wrapping_sub(INLINE_BASE) as usize;
                if let Some((written, phones)) = self.inline.get(n) {
                    new = vec![Word {
                        phones: Some(phones.clone()),
                        ..Word::new(&written.to_lowercase(), written)
                    }];
                }
            } else if let Some(w) = c.get(3) {
                let mut word = w.as_str().to_string();
                if word.starts_with('\'')
                    && !leading_apostrophe().contains(word.to_lowercase().as_str())
                {
                    out.push(Token::Punct("'".into()));
                    word.remove(0);
                } else if word.starts_with('\'') && !self.is_word(&word.to_lowercase()) {
                    word.remove(0);
                }
                let following = is_match(rx!(r"^\s*[A-Za-z]"), rest);
                new = self.word_tokens(&word, all_caps, &previous, following, fresh);
                previous = word.clone();
                if ["Ss", "ss", "Zz", "zz"].contains(&word.as_str())
                    && new.len() == 1
                    && (before.is_whitespace() || "\"'(".contains(before))
                    && is_match(rx!(r#"^[.!?…,;:—"')]*(?:\s|$)"#), rest)
                {
                    short.push(out.len());
                }
                // a reply stands free: not "(k)", "/k/", "[:k]", and not before a colon or a mid-text "!"
                if self.n.replies.contains(&word.to_lowercase())
                    && (word.chars().count() > 1 || is_lower(&word))
                    && new.len() == 1
                    && (before.is_whitespace() || before == '"' || before == '\'')
                    && is_match(rx!(r#"^(?:[.?…,;—]*["']?(?:\s|$)|!+["']?\s*$)"#), rest)
                {
                    replies.push(out.len());
                }
            } else if let Some(n) = c.get(4) {
                new = number_words(n.as_str())
                    .split(' ')
                    .map(|p| Word::new(p, n.as_str()))
                    .collect();
            } else if let Some(o) = c.get(6) {
                let other = o.as_str();
                if PUNCTUATION.contains(&other) {
                    if !(other == "'"
                        && before.is_alphanumeric()
                        && next_char.is_some_and(|n| n.is_alphanumeric()))
                    {
                        out.push(Token::Punct(other.to_string()));
                    }
                    if ".!?;…".contains(other) {
                        fresh = true;
                    }
                    attach = false;
                    continue;
                }
            }
            if attach {
                if let Some(first) = new.first_mut() {
                    first.attach = true;
                }
            }
            let any = !new.is_empty();
            out.extend(new.into_iter().map(Token::Word));
            attach = false;
            fresh = fresh && !any;
        }
        // "Ss!" and "Sh, ss, tsk" are a hiss. Beside an ordinary word the two letters stay letters.
        for place in short {
            let (mut first, mut last) = (place, place + 1);
            let part = |t: &Token| {
                matches!(t, Token::Word(_))
                    || matches!(t, Token::Punct(m) if "\"'()".contains(m.as_str()))
            };
            while first > 0 && part(&out[first - 1]) {
                first -= 1;
            }
            while last < out.len() && part(&out[last]) {
                last += 1;
            }
            let hiss = out[first..last].iter().all(|t| match t {
                Token::Word(w) => {
                    sounds().contains(w.text.as_str())
                        || (w.phones.is_some() && w.text.chars().collect::<HashSet<_>>().len() == 1)
                }
                _ => true,
            });
            if hiss {
                if let Token::Word(w) = &mut out[place] {
                    w.phones = Some(vec![(first_char(&w.text.clone()).to_string(), 0)]);
                }
            }
        }
        // "k" is "okay" as a reply by itself; beside a word or a figure it is the letter.
        for place in replies.into_iter().rev() {
            if Self::reply(&out, place) {
                let raw = match &out[place] {
                    Token::Word(w) => w.raw.clone(),
                    _ => continue,
                };
                if let Some(chat) = self.n.chat.get(&raw.to_lowercase()) {
                    let words: Vec<Token> = Self::chat_words(chat, &raw)
                        .into_iter()
                        .map(Token::Word)
                        .collect();
                    out.splice(place..place + 1, words);
                }
            }
        }
        out
    }
}
