//! Détection des zones de parole par énergie (indépendante de la langue).

use super::intervals::Interval;

/// Tranche d'analyse : 10 ms.
pub const FRAME: f64 = 0.01;

#[derive(Clone, Debug)]
pub struct SilenceParams {
    /// `None` = seuil automatique.
    pub threshold_db: Option<f64>,
    /// Une pause plus courte est gardée.
    pub min_silence: f64,
    /// Ignore les clics plus courts.
    pub min_sound: f64,
}

impl Default for SilenceParams {
    fn default() -> Self {
        Self { threshold_db: None, min_silence: 0.5, min_sound: 0.08 }
    }
}

/// Niveau en dBFS de chaque tranche de 10 ms.
pub fn frame_db(audio: &[f32], sr: u32) -> Vec<f32> {
    let hop = ((sr as f64 * FRAME) as usize).max(1);
    let n = audio.len() / hop;
    (0..n)
        .map(|i| {
            let chunk = &audio[i * hop..(i + 1) * hop];
            let ms: f64 = chunk.iter().map(|&x| (x as f64) * (x as f64)).sum::<f64>() / hop as f64;
            (20.0 * (ms.sqrt() + 1e-10).log10()) as f32
        })
        .collect()
}

/// Percentile à interpolation linéaire (comme numpy.percentile).
fn percentile(sorted: &[f32], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let pos = p / 100.0 * (sorted.len() - 1) as f64;
    let lo = pos.floor() as usize;
    let hi = pos.ceil() as usize;
    let frac = pos - lo as f64;
    sorted[lo] as f64 + (sorted[hi] as f64 - sorted[lo] as f64) * frac
}

/// Seuil placé entre le bruit de fond (percentile 10) et la voix (percentile 95).
pub fn auto_threshold(db: &[f32]) -> f64 {
    if db.is_empty() {
        return -45.0;
    }
    let mut s = db.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let floor = percentile(&s, 10.0).max(-80.0);
    let peak = percentile(&s, 95.0);
    let thr = floor + 0.3 * (peak - floor);
    // np.clip(thr, -60, peak - 6) : le maximum gagne si les bornes se croisent.
    thr.max(-60.0).min(peak - 6.0)
}

/// Plages [début, fin) où `mask` est vrai.
fn runs(mask: &[bool]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start = None;
    for (i, &m) in mask.iter().enumerate() {
        match (m, start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                out.push((s, i));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        out.push((s, mask.len()));
    }
    out
}

/// Arrondi « au pair » comme `round()` de Python.
pub fn pyround(x: f64) -> f64 {
    x.round_ties_even()
}

/// Zones sonores (secondes) et seuil utilisé (dBFS), à partir des dB par tranche.
pub fn detect_speech_db(db: &[f32], params: &SilenceParams) -> (Vec<Interval>, f64) {
    let thr = params.threshold_db.unwrap_or_else(|| auto_threshold(db));
    let mut loud: Vec<bool> = db.iter().map(|&d| (d as f64) > thr).collect();

    // Les pauses courtes font partie de la phrase.
    let min_sil = pyround(params.min_silence / FRAME) as usize;
    let quiet: Vec<bool> = loud.iter().map(|l| !l).collect();
    for (s, e) in runs(&quiet) {
        if e - s < min_sil && s > 0 && e < loud.len() {
            loud[s..e].iter_mut().for_each(|x| *x = true);
        }
    }

    let min_snd = pyround(params.min_sound / FRAME) as usize;
    let speech = runs(&loud)
        .into_iter()
        .filter(|(s, e)| e - s >= min_snd)
        .map(|(s, e)| (s as f64 * FRAME, e as f64 * FRAME))
        .collect();
    (speech, thr)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_tone_between_silences() {
        let sr = 16000;
        let mut audio = vec![0.0f32; sr as usize * 3];
        for (i, x) in audio.iter_mut().enumerate().skip(sr as usize).take(sr as usize) {
            *x = 0.5 * ((i as f32) * 0.05).sin();
        }
        let db = frame_db(&audio, sr);
        let (speech, _) = detect_speech_db(&db, &SilenceParams::default());
        assert_eq!(speech.len(), 1);
        assert!((speech[0].0 - 1.0).abs() < 0.02 && (speech[0].1 - 2.0).abs() < 0.02);
    }
}
