/// The discrete kinds of goal FRIDAY can currently understand. The set is small
/// and rule-based on purpose: LLM-based classification plugs in here later
/// without changing any downstream code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntentType {
    BrowserSearch,
    OpenApp,
    RoutineMorning,
    Unknown,
}

impl IntentType {
    /// Classify free text into an `IntentType` using simple keyword rules.
    /// This is the swappable seam for a future model-based classifier.
    pub fn classify(raw: &str) -> Self {
        let r = raw.to_ascii_lowercase();
        if r.contains("morning routine") || (r.contains("morning") && r.contains("routine")) {
            IntentType::RoutineMorning
        } else if r.contains("search")
            && (r.contains("browser")
                || r.contains("chrome")
                || r.contains("edge")
                || r.contains("firefox")
                || r.contains("web")
                || r.contains("google")
                || r.contains("duckduckgo")
                || r.contains("bing"))
        {
            IntentType::BrowserSearch
        } else if r.contains("open") {
            IntentType::OpenApp
        } else {
            IntentType::Unknown
        }
    }

    pub fn as_intent(&self) -> &'static str {
        match self {
            IntentType::BrowserSearch => "browser.search",
            IntentType::OpenApp => "app.open",
            IntentType::RoutineMorning => "routine.morning",
            IntentType::Unknown => "unknown",
        }
    }
}

/// Convenience free function wrapping [`IntentType::classify`].
pub fn classify(raw: &str) -> IntentType {
    IntentType::classify(raw)
}
