//! Calcule les segments à garder à partir des zones sonores et des mots transcrits.

use super::fillers::is_cuttable;
use super::retakes::{find_retakes, Retake};
use super::intervals::{invert, merge, pad, subtract, total, Interval};
use super::silence::pyround;
use super::transcribe::Word;
use super::Fps;
use std::collections::HashSet;

#[derive(Clone, Debug)]
pub struct CutParams {
    /// Marge gardée avant/après chaque phrase.
    pub margin: f64,
    /// Un segment gardé plus court est supprimé.
    pub min_keep: f64,
    pub cut_fillers: bool,
    /// Bâillements, toux, souffles ([yawn], [cough]…).
    pub cut_noises: bool,
    /// Marge autour d'un « euh » collé à un mot.
    pub filler_pad: f64,
    /// La coupe d'un « euh » ne déborde pas plus loin.
    pub max_filler_extend: f64,
    pub extra_fillers: HashSet<String>,
    /// Phrases ratées puis recommencées, bégaiements : on garde la dernière tentative.
    pub cut_retakes: bool,
}

impl Default for CutParams {
    fn default() -> Self {
        Self {
            margin: 0.15,
            min_keep: 0.25,
            cut_fillers: true,
            cut_noises: true,
            filler_pad: 0.03,
            max_filler_extend: 1.0,
            extra_fillers: HashSet::new(),
            cut_retakes: true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Keep,
    Silence,
    Filler,
    /// Tentative ratée d'une phrase recommencée juste après.
    Retake,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Keep => "keep",
            Kind::Silence => "silence",
            Kind::Filler => "filler",
            Kind::Retake => "retake",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Region {
    pub start: f64,
    pub end: f64,
    pub kind: Kind,
    pub text: String,
}

#[derive(Clone, Debug, Default)]
pub struct Stats {
    pub duration: f64,
    pub kept: f64,
    pub removed: f64,
    pub removed_pct: f64,
    pub segments: usize,
    pub fillers: usize,
    pub retakes: usize,
}

#[derive(Clone, Debug)]
pub struct CutResult {
    pub duration: f64,
    pub keep: Vec<Interval>,
    pub regions: Vec<Region>,
    pub fillers: Vec<Word>,
    pub retakes: Vec<Retake>,
}

fn round_to(x: f64, digits: i32) -> f64 {
    let m = 10f64.powi(digits);
    pyround(x * m) / m
}

impl CutResult {
    pub fn stats(&self) -> Stats {
        let kept = total(&self.keep);
        let removed = self.duration - kept;
        Stats {
            duration: round_to(self.duration, 3),
            kept: round_to(kept, 3),
            removed: round_to(removed, 3),
            removed_pct: if self.duration > 0.0 { round_to(100.0 * removed / self.duration, 1) } else { 0.0 },
            segments: self.keep.len(),
            fillers: self.fillers.len(),
            retakes: self.retakes.len(),
        }
    }
}

/// Pour chaque « euh », coupe aussi les pauses qui l'entourent, en gardant une marge naturelle.
pub fn filler_cuts(words: &[Word], duration: f64, p: &CutParams) -> (Vec<Interval>, Vec<Word>) {
    let flags: Vec<bool> = words.iter().map(|w| is_cuttable(&w.word, &p.extra_fillers, p.cut_noises)).collect();
    let mut cuts = Vec::new();
    let mut fillers = Vec::new();
    for (i, w) in words.iter().enumerate() {
        if !flags[i] {
            continue;
        }
        fillers.push(w.clone());
        let prev_end = (0..i).rev().find(|&j| !flags[j]).map(|j| words[j].end).unwrap_or(0.0);
        let next_start = (i + 1..words.len()).find(|&j| !flags[j]).map(|j| words[j].start).unwrap_or(duration);

        let mut left = if prev_end + p.margin < w.start { prev_end + p.margin } else { prev_end.max(w.start - p.filler_pad) };
        let mut right = if next_start - p.margin > w.end { next_start - p.margin } else { next_start.min(w.end + p.filler_pad) };
        left = left.max(w.start - p.max_filler_extend);
        right = right.min(w.end + p.max_filler_extend);
        cuts.push((left, right));
    }
    (merge(&cuts), fillers)
}

pub fn compute_cuts(duration: f64, speech: &[Interval], words: Option<&[Word]>, p: &CutParams) -> CutResult {
    let mut keep = pad(speech, p.margin, 0.0, duration);

    let mut cuts = Vec::new();
    let mut fillers = Vec::new();
    let mut retakes = Vec::new();
    if let Some(words) = words {
        if !words.is_empty() && p.cut_fillers {
            let (c, f) = filler_cuts(words, duration, p);
            cuts = c;
            fillers = f;
            keep = subtract(&keep, &cuts);
        }
        if !words.is_empty() && p.cut_retakes {
            retakes = find_retakes(words);
            let r: Vec<Interval> = retakes.iter().map(|r| (r.start, r.end)).collect();
            keep = subtract(&keep, &merge(&r));
        }
    }

    let keep: Vec<Interval> = merge(&keep).into_iter().filter(|(s, e)| e - s >= p.min_keep).collect();

    let mut regions: Vec<Region> = keep.iter().map(|&(s, e)| Region { start: s, end: e, kind: Kind::Keep, text: String::new() }).collect();
    // Une coupe qui touche une reprise est découpée : silence avant / reprise / silence après,
    // pour que garder la reprise à la main ne ramène pas aussi le silence voisin.
    let mut pieces: Vec<(f64, f64, bool)> = Vec::new();
    for (s, e) in invert(&keep, 0.0, duration) {
        let over: Vec<&Retake> = retakes.iter().filter(|r| r.start < e && r.end > s).collect();
        if over.is_empty() {
            pieces.push((s, e, false));
            continue;
        }
        let rs = over.iter().map(|r| r.start).fold(f64::INFINITY, f64::min).max(s);
        let re = over.iter().map(|r| r.end).fold(f64::NEG_INFINITY, f64::max).min(e);
        if rs - s > 0.05 {
            pieces.push((s, rs, false));
        }
        pieces.push((rs, re, true));
        if e - re > 0.05 {
            pieces.push((re, e, false));
        }
    }
    for (s, e, is_retake) in pieces {
        if is_retake {
            let text = retakes.iter().filter(|r| r.start < e && r.end > s).map(|r| r.text.as_str()).collect::<Vec<_>>().join(" … ");
            regions.push(Region { start: s, end: e, kind: Kind::Retake, text });
            continue;
        }
        let is_fill = cuts.iter().any(|&(cs, ce)| cs < e && ce > s);
        let text = if is_fill {
            fillers.iter().filter(|f| s <= f.start && f.start < e).map(|f| f.word.as_str()).collect::<Vec<_>>().join(" ")
        } else {
            String::new()
        };
        regions.push(Region { start: s, end: e, kind: if is_fill { Kind::Filler } else { Kind::Silence }, text });
    }
    regions.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap_or(std::cmp::Ordering::Equal));
    CutResult { duration, keep, regions, fillers, retakes }
}

/// Aligne les segments sur les images de la vidéo : [début, fin) en numéros d'image.
pub fn to_frames(keep: &[Interval], fps: Fps, total_frames: i64) -> Vec<(i64, i64)> {
    let f = fps.as_f64();
    let mut out: Vec<(i64, i64)> = Vec::new();
    for &(s, e) in keep {
        let a = (pyround(s * f) as i64).max(0);
        let b = (pyround(e * f) as i64).min(total_frames);
        if b <= a {
            continue;
        }
        if let Some(last) = out.last_mut() {
            if a <= last.1 {
                last.1 = last.1.max(b);
                continue;
            }
        }
        out.push((a, b));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filler_is_cut_with_pauses() {
        let words = vec![
            Word { word: "Bonjour".into(), start: 0.5, end: 1.0 },
            Word { word: "euh".into(), start: 1.5, end: 1.9 },
            Word { word: "tout".into(), start: 2.6, end: 3.0 },
        ];
        let r = compute_cuts(4.0, &[(0.4, 3.1)], Some(&words), &CutParams::default());
        assert!(r.regions.iter().any(|g| g.kind == Kind::Filler && g.text == "euh"));
        assert_eq!(r.keep.len(), 2);
        assert!((r.keep[0].1 - 1.15).abs() < 1e-9 && (r.keep[1].0 - 2.45).abs() < 1e-9);
    }

    #[test]
    fn retake_is_cut_and_labelled() {
        // « on va on va ouvrir » : la première tentative « on va » est coupée
        let w = |t: &str, s: f64| Word { word: t.into(), start: s, end: s + 0.25 };
        let words = vec![w("On", 1.0), w("va", 1.3), w("on", 1.6), w("va", 1.9), w("ouvrir", 2.2)];
        let r = compute_cuts(3.0, &[(0.9, 2.5)], Some(&words), &CutParams::default());
        let rt: Vec<&Region> = r.regions.iter().filter(|g| g.kind == Kind::Retake).collect();
        assert_eq!(rt.len(), 1);
        assert_eq!(rt[0].text, "On va");
        assert!(rt[0].start <= 1.0 && (rt[0].end - 1.57).abs() < 1e-9);
        assert_eq!(r.stats().retakes, 1);
        let off = compute_cuts(3.0, &[(0.9, 2.5)], Some(&words), &CutParams { cut_retakes: false, ..Default::default() });
        assert!(off.regions.iter().all(|g| g.kind != Kind::Retake));
    }
}
