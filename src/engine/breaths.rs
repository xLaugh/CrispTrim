//! Respirations entre les mots : souffle sans note de voix, plus fort qu'un silence, plus faible que
//! la parole. La transcription dit où sont les mots ; on examine le son des trous entre eux.
//!
//! Réglé sur un vrai rush : respirations à taux de passages par zéro ≥ 0,18 (souffle), voisement
//! ≤ 0,45 (pas de note de voix), crête ≤ −33 dBFS, de 50 ms à 1 s. Les fins de mots non
//! transcrites, elles, ont de la voix (voisement 0,6 à 0,99) et sont gardées.

use super::intervals::Interval;
use super::silence::FRAME;
use super::transcribe::Word;

/// Mesures par tranche de 10 ms (fenêtre de 25 ms).
#[derive(Clone, Debug, Default)]
pub struct VoiceFrames {
    /// Taux de passages par zéro (0..1) : élevé pour un souffle.
    pub zcr: Vec<f32>,
    /// Voisement (0..1) : autocorrélation maximale pour une note de 80 à 400 Hz.
    pub voicing: Vec<f32>,
}

const MIN_ZCR: f32 = 0.18;
const MAX_VOICING: f32 = 0.45;
const MAX_PEAK_DB: f32 = -33.0;
const MIN_FRAMES: usize = 5; // 50 ms
const MAX_LEN: f64 = 1.0;
/// Les mots débordent un peu : on laisse 30 ms à chaque bord du trou.
const GUARD: f64 = 0.03;

/// Mesures de voix et de souffle d'un signal (16 kHz attendu).
pub fn voice_frames(audio: &[f32], sr: u32) -> VoiceFrames {
    let hop = ((sr as f64 * FRAME) as usize).max(1);
    let win = hop * 5 / 2;
    let n = audio.len() / hop;
    // voisement calculé à mi-fréquence (8 kHz) : 4 fois moins de calcul, même résultat pour 80-400 Hz
    let half: Vec<f32> = audio.iter().step_by(2).copied().collect();
    let (hwin, lag_min, lag_max) = (win / 2, (sr as usize / 2) / 400, (sr as usize / 2) / 80);
    let mut zcr = Vec::with_capacity(n);
    let mut voicing = Vec::with_capacity(n);
    for i in 0..n {
        let s = i * hop;
        let seg = &audio[s..(s + win).min(audio.len())];
        let z = if seg.len() > 1 {
            seg.windows(2).filter(|w| (w[0] >= 0.0) != (w[1] >= 0.0)).count() as f32 / (seg.len() - 1) as f32
        } else {
            0.0
        };
        zcr.push(z);
        let hs = s / 2;
        let x = &half[hs.min(half.len())..(hs + hwin).min(half.len())];
        let mut best = 0f32;
        if x.len() > lag_max + 8 {
            let mean = x.iter().sum::<f32>() / x.len() as f32;
            let x: Vec<f32> = x.iter().map(|v| v - mean).collect();
            for lag in lag_min..=lag_max {
                let (u, v) = (&x[..x.len() - lag], &x[lag..]);
                let (mut uv, mut uu, mut vv) = (0f32, 0f32, 0f32);
                for k in 0..u.len() {
                    uv += u[k] * v[k];
                    uu += u[k] * u[k];
                    vv += v[k] * v[k];
                }
                let r = uv / ((uu * vv).sqrt() + 1e-12);
                best = best.max(r);
            }
        }
        voicing.push(best);
    }
    VoiceFrames { zcr, voicing }
}

/// Respirations entre les mots, en secondes. `thr` : seuil de silence (dBFS).
pub fn detect(db: &[f32], vf: &VoiceFrames, words: &[Word], thr: f64) -> Vec<Interval> {
    let n = db.len().min(vf.zcr.len()).min(vf.voicing.len());
    let fr = |t: f64| ((t / FRAME).floor().max(0.0) as usize).min(n);
    let mut out = Vec::new();
    for pair in words.windows(2) {
        let (gs, ge) = (pair[0].end + GUARD, pair[1].start - GUARD);
        if ge - gs < MIN_FRAMES as f64 * FRAME {
            continue;
        }
        let (g0, g1) = (fr(gs), fr(ge));
        let mut i = g0;
        while i < g1 {
            if (db[i] as f64) <= thr {
                i += 1;
                continue;
            }
            // passage sonore (avec 6 dB de tolérance pour ne pas le hacher)
            let mut j = i;
            while j < g1 && (db[j] as f64) > thr - 6.0 {
                j += 1;
            }
            if j - i >= MIN_FRAMES {
                let len = (j - i) as f32;
                let z = vf.zcr[i..j].iter().sum::<f32>() / len;
                let v = vf.voicing[i..j].iter().sum::<f32>() / len;
                let peak = db[i..j].iter().cloned().fold(f32::MIN, f32::max);
                if z >= MIN_ZCR && v <= MAX_VOICING && peak <= MAX_PEAK_DB && (j - i) as f64 * FRAME <= MAX_LEN {
                    let s = (i as f64 * FRAME - GUARD).max(pair[0].end);
                    let e = (j as f64 * FRAME + GUARD).min(pair[1].start);
                    out.push((s, e));
                }
            }
            i = j.max(i + 1);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::silence::frame_db;

    fn noise(n: usize, amp: f32, seed: &mut u64) -> Vec<f32> {
        (0..n)
            .map(|_| {
                *seed ^= *seed << 13;
                *seed ^= *seed >> 7;
                *seed ^= *seed << 17;
                ((*seed % 2000) as f32 / 1000.0 - 1.0) * amp
            })
            .collect()
    }

    /// mot (voix 150 Hz) · respiration (souffle) · mot · silence · mot (voix)
    fn sample() -> (Vec<f32>, Vec<Word>) {
        let sr = 16000.0;
        let mut seed = 7u64;
        let voice = |n: usize| (0..n).map(|i| 0.3 * (2.0 * std::f32::consts::PI * 150.0 * i as f32 / sr).sin()).collect::<Vec<_>>();
        let mut a = Vec::new();
        a.extend(voice(8000)); // 0,0 – 0,5 mot
        a.extend(noise(4800, 0.004, &mut seed)); // 0,5 – 0,8 respiration (≈ −50 dB)
        a.extend(voice(8000)); // 0,8 – 1,3 mot
        a.extend(vec![0.0; 4800]); // 1,3 – 1,6 silence
        a.extend(voice(8000)); // 1,6 – 2,1 mot
        let w = |t: &str, s: f64, e: f64| Word { word: t.into(), start: s, end: e };
        (a, vec![w("un", 0.0, 0.5), w("deux", 0.8, 1.3), w("trois", 1.6, 2.1)])
    }

    #[test]
    fn voice_and_breath_measures() {
        let (a, _) = sample();
        let vf = voice_frames(&a, 16000);
        let avg = |v: &[f32], a: usize, b: usize| v[a..b].iter().sum::<f32>() / (b - a) as f32;
        assert!(avg(&vf.voicing, 10, 40) > 0.9, "voix");
        assert!(avg(&vf.voicing, 55, 75) < 0.45, "souffle sans voix");
        assert!(avg(&vf.zcr, 55, 75) > 0.3 && avg(&vf.zcr, 10, 40) < 0.05);
    }

    #[test]
    fn breath_found_silence_ignored() {
        let (a, words) = sample();
        let db = frame_db(&a, 16000);
        let found = detect(&db, &voice_frames(&a, 16000), &words, -60.0);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].0 >= 0.5 && found[0].0 < 0.56 && found[0].1 > 0.74 && found[0].1 <= 0.8, "{found:?}");
    }

    #[test]
    fn loud_unvoiced_is_kept() {
        // souffle fort (consonne « ch » non transcrite) : gardé
        let (mut a, words) = sample();
        let mut seed = 3u64;
        let loud = noise(4800, 0.2, &mut seed);
        a[8000..12800].copy_from_slice(&loud);
        let db = frame_db(&a, 16000);
        assert!(detect(&db, &voice_frames(&a, 16000), &words, -60.0).is_empty());
    }
}
