//! Exports texte : transcription (.txt) et sous-titres (.srt) recalés sur le montage.

use super::fillers::is_cuttable;
use super::intervals::Interval;
use super::transcribe::Word;
use regex::Regex;
use std::collections::HashSet;
use std::path::Path;
use std::sync::OnceLock;

fn re(i: usize) -> &'static Regex {
    static R: OnceLock<[Regex; 4]> = OnceLock::new();
    &R.get_or_init(|| {
        [
            Regex::new(r"\s+([,.;:!?…])").unwrap(),
            Regex::new(r"\s{2,}").unwrap(),
            Regex::new(r"[.!?…]$").unwrap(),
            Regex::new(r"[,;:]$").unwrap(),
        ]
    })[i]
}

fn clen(s: &str) -> usize {
    s.chars().count()
}

pub fn clean(text: &str, capitalize: bool) -> String {
    let t = re(0).replace_all(text, "$1");
    let t = re(1).replace_all(&t, " ");
    let t = t.trim();
    if !capitalize {
        return t.to_string();
    }
    let mut c = t.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// Mots prononcés qui restent dans le montage (sans hésitations ni bruits).
pub fn kept_words(words: &[Word], keep: &[Interval]) -> Vec<Word> {
    let none = HashSet::new();
    let mut out = Vec::new();
    let mut k = 0;
    for w in words {
        if is_cuttable(&w.word, &none, true) {
            continue;
        }
        let mid = (w.start + w.end) / 2.0;
        while k < keep.len() && keep[k].1 < mid {
            k += 1;
        }
        if k < keep.len() && keep[k].0 <= mid && mid <= keep[k].1 {
            out.push(w.clone());
        }
    }
    out
}

/// Temps dans la vidéo d'origine → temps dans le montage.
pub fn retime(t: f64, keep: &[Interval]) -> f64 {
    let mut acc = 0.0;
    for &(s, e) in keep {
        if t <= s {
            return acc;
        }
        if t <= e {
            return acc + t - s;
        }
        acc += e - s;
    }
    acc
}

pub fn paragraphs(words: &[Word], gap: f64) -> Vec<(f64, String)> {
    let mut paras: Vec<(f64, Vec<&str>)> = Vec::new();
    let mut prev: Option<f64> = None;
    for w in words {
        if paras.is_empty() || prev.map(|p| w.start - p > gap).unwrap_or(false) {
            paras.push((w.start, Vec::new()));
        }
        paras.last_mut().unwrap().1.push(&w.word);
        prev = Some(w.end);
    }
    paras.into_iter().map(|(s, ws)| (s, clean(&ws.join(" "), true))).collect()
}

pub fn txt_content(words: &[Word], keep: &[Interval], timestamps: bool) -> String {
    let lines: Vec<String> = paragraphs(&kept_words(words, keep), 1.5)
        .into_iter()
        .map(|(start, text)| {
            let t = retime(start, keep);
            if timestamps {
                format!("[{:02}:{:02}] {}", (t / 60.0).floor() as i64, (t % 60.0).floor() as i64, text)
            } else {
                text
            }
        })
        .collect();
    lines.join("\n\n") + "\n"
}

pub fn write_txt(words: &[Word], keep: &[Interval], out: &Path) -> std::io::Result<()> {
    super::write_text(out, &txt_content(words, keep, true))
}

fn srt_time(t: f64) -> String {
    let ms = (t.max(0.0) * 1000.0).round_ties_even() as i64;
    let (h, ms) = (ms / 3_600_000, ms % 3_600_000);
    let (m, ms) = (ms / 60_000, ms % 60_000);
    let (s, ms) = (ms / 1000, ms % 1000);
    format!("{h:02}:{m:02}:{s:02},{ms:03}")
}

fn two_lines(text: &str, width: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= width {
        return text.to_string();
    }
    let mid = chars.len() / 2;
    let cut = chars
        .iter()
        .enumerate()
        .filter(|(_, c)| **c == ' ')
        .map(|(i, _)| i)
        .min_by_key(|&i| (i as i64 - mid as i64).abs());
    match cut {
        None => text.to_string(),
        Some(c) => format!("{}\n{}", chars[..c].iter().collect::<String>(), chars[c + 1..].iter().collect::<String>()),
    }
}

/// Regroupe les mots en sous-titres (2 lignes max), calés sur le montage.
pub fn subtitle_cues(words: &[Word], keep: &[Interval], width: usize, max_dur: f64, gap: f64) -> Vec<(f64, f64, String)> {
    let mut cues: Vec<(f64, f64, String)> = Vec::new();
    let mut cur: Vec<Word> = Vec::new();
    let mut sentence_start = true;
    let joined = |c: &[Word]| c.iter().map(|w| w.word.as_str()).collect::<Vec<_>>().join(" ");

    let flush = |cur: &mut Vec<Word>, cues: &mut Vec<(f64, f64, String)>, sentence_start: &mut bool| {
        if !cur.is_empty() {
            let text = clean(&joined(cur), *sentence_start);
            cues.push((retime(cur[0].start, keep), retime(cur[cur.len() - 1].end, keep), two_lines(&text, width)));
            *sentence_start = re(2).is_match(&cur[cur.len() - 1].word);
            cur.clear();
        }
    };

    for w in kept_words(words, keep) {
        if !cur.is_empty() {
            let length = clen(&joined(&cur)) + 1 + clen(&w.word);
            let last = &cur[cur.len() - 1];
            // Les écarts se mesurent dans le montage : les pauses coupées ne comptent plus.
            let pause = retime(w.start, keep) - retime(last.end, keep);
            let span = retime(w.end, keep) - retime(cur[0].start, keep);
            // Au-delà d'une ligne, on coupe de préférence sur une pause de la voix ou une virgule.
            let cur_len = length - 1 - clen(&w.word);
            let natural = w.start - last.end > 0.3 || re(3).is_match(&last.word);
            if pause > gap || length > 2 * width || span > max_dur || (cur_len >= width && natural) {
                flush(&mut cur, &mut cues, &mut sentence_start);
            }
        }
        let ends = re(2).is_match(&w.word);
        cur.push(w);
        if ends && clen(&joined(&cur)) as f64 > width as f64 / 2.0 {
            flush(&mut cur, &mut cues, &mut sentence_start);
        }
    }
    flush(&mut cur, &mut cues, &mut sentence_start);

    // Pas de chevauchement, durée minimale lisible.
    let mut fixed = Vec::with_capacity(cues.len());
    for i in 0..cues.len() {
        let (s, e, ref t) = cues[i];
        let mut e = e.max(s + 0.7);
        if let Some(n) = cues.get(i + 1) {
            e = e.min(n.0 - 0.04);
        }
        fixed.push((s, e.max(s + 0.1), t.clone()));
    }
    fixed
}

pub fn srt_content(words: &[Word], keep: &[Interval]) -> String {
    subtitle_cues(words, keep, 42, 5.0, 0.8)
        .iter()
        .enumerate()
        .map(|(i, (s, e, t))| format!("{}\n{} --> {}\n{}\n", i + 1, srt_time(*s), srt_time(*e), t))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn write_srt(words: &[Word], keep: &[Interval], out: &Path) -> std::io::Result<()> {
    super::write_text(out, &srt_content(words, keep))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(word: &str, s: f64, e: f64) -> Word {
        Word { word: word.into(), start: s, end: e }
    }

    #[test]
    fn text_skips_fillers_and_retimes() {
        let words = vec![w("bonjour", 0.0, 0.5), w("euh", 0.6, 0.9), w("à", 1.0, 1.1), w("tous", 1.1, 1.4), w(".", 1.4, 1.4)];
        let keep = vec![(0.0, 0.55), (0.95, 1.5)];
        assert_eq!(txt_content(&words, &keep, true), "[00:00] Bonjour à tous.\n");
        assert!((retime(1.0, &keep) - 0.6).abs() < 1e-9);
        assert!(srt_content(&words, &keep).starts_with("1\n00:00:00,000 --> "));
    }
}
