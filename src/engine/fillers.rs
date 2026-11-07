//! Reconnaissance des hésitations en français (« euh », « hum »…) et des marqueurs CrisperWhisper.

use regex::Regex;
use std::collections::HashSet;
use std::sync::OnceLock;

/// Marqueurs produits par CrisperWhisper en mode verbatim.
const MARKERS: [&str; 5] = ["[uh]", "[um]", "[uhm]", "[hm]", "[mm]"];

/// Événements sonores à ne jamais couper ; les autres ([yawn], [cough]…) sont des bruits.
const KEEP_EVENTS: [&str; 6] = ["[laughter]", "[laugh]", "[laughs]", "[music]", "[applause]", "[singing]"];

fn filler_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // Volontairement prudent : « eu », « bah », « ben », « hein » sont de vrais mots.
    RE.get_or_init(|| {
        Regex::new(r"^(?:e+u+h+|h+e+u+h*|e+u+h*m+|h+u+m+|h+m+|m+h+|m{2,}|u+h+|u+m+)$").unwrap()
    })
}

pub fn normalize(word: &str) -> String {
    let w = word.trim().to_lowercase();
    if w.starts_with('[') && w.ends_with(']') {
        return w;
    }
    w.chars().filter(|c| c.is_alphanumeric() || *c == '_').collect()
}

/// Bruit de bouche transcrit entre crochets (bâillement, toux, souffle…).
pub fn is_noise(word: &str) -> bool {
    let w = normalize(word);
    w.starts_with('[') && w.ends_with(']') && !MARKERS.contains(&w.as_str()) && !KEEP_EVENTS.contains(&w.as_str())
}

pub fn is_filler(word: &str, extra: &HashSet<String>) -> bool {
    let w = normalize(word);
    if w.is_empty() {
        return false;
    }
    if MARKERS.contains(&w.as_str()) || extra.contains(&w) {
        return true;
    }
    filler_re().is_match(&w)
}

pub fn is_cuttable(word: &str, extra: &HashSet<String>, cut_noises: bool) -> bool {
    is_filler(word, extra) || (cut_noises && is_noise(word))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn french_fillers() {
        let none = HashSet::new();
        for w in ["euh", "Euuh,", "heu", "hum", "mmh", "hmm", "[uh]", "euhm"] {
            assert!(is_filler(w, &none), "{w}");
        }
        for w in ["eu", "bah", "ben", "hein", "maman", "[laughter]"] {
            assert!(!is_filler(w, &none), "{w}");
        }
        assert!(is_noise("[cough]") && !is_noise("[laughs]"));
    }
}
