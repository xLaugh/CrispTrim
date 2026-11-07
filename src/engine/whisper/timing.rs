//! Horodatages mot à mot à partir de l'attention croisée : port de `crisperwhisper/word_timing.py`
//! (regroupement des jetons en mots, « blanc » tiré de l'énergie du mel, Viterbi mot à mot).

pub const FRAME_DURATION: f64 = 0.02;
pub const SPACE_TOKEN: u32 = 220;

/// Paramètres par défaut de `extract_word_timings` (chemin suivi par CrispTrim).
pub const SHARPEN: f64 = 5.0;
pub const BLANK_GAMMA: f64 = 3.0;
pub const BLANK_PENALTY: f64 = 3.0;
pub const SPLIT_GAP_MAX: f64 = 0.1;

#[derive(Clone, Debug, PartialEq)]
pub struct Timed {
    pub word: String,
    pub start: f64,
    pub end: f64,
}

fn is_special_piece(p: &str) -> bool {
    (p.starts_with("<|") && p.ends_with("|>"))
        || p.starts_with("[verbatim_")
        || p.starts_with("[intended_")
        || matches!(p, "<ctx>" | "<ectx>" | "<htx>" | "<ehtx>" | "<vtx>" | "<evtx>" | "<sot>" | "<eot>")
}

/// Jetons → mots (indices des jetons de chaque mot), comme `group_tokens_into_words`.
pub fn group_tokens_into_words(ids: &[u32], pieces: &[String]) -> Vec<Vec<usize>> {
    let mut words = Vec::new();
    let (mut cur, mut text): (Vec<usize>, String) = (Vec::new(), String::new());
    let flush = |cur: &mut Vec<usize>, text: &mut String, words: &mut Vec<Vec<usize>>| {
        if !text.trim().is_empty() && !cur.is_empty() {
            words.push(std::mem::take(cur));
        }
        cur.clear();
        text.clear();
    };
    for (i, (&id, p)) in ids.iter().zip(pieces).enumerate() {
        let space = id == SPACE_TOKEN || p == " ";
        if is_special_piece(p) || space {
            flush(&mut cur, &mut text, &mut words);
            continue;
        }
        if p.starts_with(' ') && !text.is_empty() {
            flush(&mut cur, &mut text, &mut words);
        }
        cur.push(i);
        text.push_str(p);
    }
    flush(&mut cur, &mut text, &mut words);
    words
}

/// Attention [T, F] (après softmax) → log-probabilités par jeton, avec affûtage.
pub fn token_logp(attn: &[Vec<f32>], sharpen: f64) -> Vec<Vec<f32>> {
    attn.iter()
        .map(|row| {
            let a: Vec<f64> = row.iter().map(|&x| (x.max(0.0) as f64).powf(sharpen)).collect();
            let s = a.iter().sum::<f64>().max(1e-8);
            a.iter().map(|&x| ((x / s) + 1e-8).ln() as f32).collect()
        })
        .collect()
}

fn percentile(v: &[f32], q: f64) -> f64 {
    // np.percentile, interpolation linéaire
    let mut s: Vec<f64> = v.iter().map(|&x| x as f64).collect();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap());
    if s.is_empty() {
        return 0.0;
    }
    let pos = q / 100.0 * (s.len() - 1) as f64;
    let (lo, hi) = (pos.floor() as usize, pos.ceil() as usize);
    s[lo] + (s[hi] - s[lo]) * (pos - lo as f64)
}

fn resample(x: &[f32], n: usize) -> Vec<f32> {
    if n == 0 {
        return vec![];
    }
    if x.len() == n {
        return x.to_vec();
    }
    if x.len() <= 1 {
        return vec![x.first().copied().unwrap_or(0.0); n];
    }
    // np.interp sur np.linspace(0, 1) des deux côtés (calcul en float32 comme numpy)
    let m = x.len();
    (0..n)
        .map(|i| {
            let t = if n == 1 { 0.0 } else { i as f32 / (n - 1) as f32 };
            let p = t * (m - 1) as f32;
            let lo = (p.floor() as usize).min(m - 1);
            let hi = (lo + 1).min(m - 1);
            let src_lo = lo as f32 / (m - 1) as f32;
            let src_hi = hi as f32 / (m - 1) as f32;
            if hi == lo || src_hi == src_lo {
                x[lo]
            } else {
                x[lo] + (x[hi] - x[lo]) * (t - src_lo) / (src_hi - src_lo)
            }
        })
        .collect()
}

/// « Blanc » (silence) par position de l'encodeur, tiré de l'énergie du mel [n_mels][n_frames].
pub fn blank_logp_from_mel(mel: &[Vec<f32>], target: usize, gamma: f64, penalty: f64) -> Vec<f32> {
    let n_frames = mel.first().map_or(0, |r| r.len());
    let n_mels = mel.len().max(1) as f32;
    let energy: Vec<f32> = (0..n_frames).map(|f| mel.iter().map(|r| r[f]).sum::<f32>() / n_mels).collect();
    let (p10, p90) = (percentile(&energy, 10.0), percentile(&energy, 90.0));
    let denom = (p90 - p10).max(1e-6);
    let norm: Vec<f32> = energy.iter().map(|&e| (((e as f64 - p10) / denom).clamp(0.0, 1.0)) as f32).collect();
    resample(&norm, target)
        .iter()
        .map(|&e| {
            let mut p = (1.0 - e as f64).clamp(1e-4, 1.0 - 1e-4);
            if gamma != 1.0 {
                p = p.powf(gamma).clamp(1e-4, 1.0);
            }
            ((p + 1e-6).ln() - penalty) as f32
        })
        .collect()
}

fn logaddexp(a: f32, b: f32) -> f32 {
    let m = a.max(b);
    if m == f32::NEG_INFINITY {
        return m;
    }
    m + ((a - m).exp() + (b - m).exp()).ln()
}

/// Viterbi avec blancs virtuels [blanc0, mot0, blanc1, …] ; (début, fin) par mot, en secondes.
pub fn viterbi_words(tok_logp: &[Vec<f32>], blank: &[f32], words: &[Vec<usize>]) -> Vec<Option<(f64, f64)>> {
    let f_len = blank.len();
    let w_len = words.len();
    if w_len == 0 || f_len == 0 {
        return vec![];
    }
    let word_logp: Vec<Vec<f32>> = words
        .iter()
        .map(|idx| {
            let valid: Vec<&Vec<f32>> = idx.iter().filter_map(|&i| tok_logp.get(i)).collect();
            if valid.is_empty() {
                return vec![-1e9; f_len];
            }
            (0..f_len).map(|f| valid.iter().fold(f32::NEG_INFINITY, |acc, r| logaddexp(acc, r[f]))).collect()
        })
        .collect();
    let s_len = 2 * w_len + 1;
    let emit = |s: usize, f: usize| if s % 2 == 0 { blank[f] } else { word_logp[s / 2][f] };
    let neg = -1e9f32;
    let mut prev = vec![neg; s_len];
    prev[0] = emit(0, 0);
    let mut back = vec![vec![false; s_len]; f_len];
    for f in 1..f_len {
        let mut cur = vec![neg; s_len];
        for s in 0..s_len {
            let stay = prev[s];
            let adv = if s > 0 { prev[s - 1] } else { neg };
            let take = adv > stay;
            cur[s] = if take { adv } else { stay } + emit(s, f);
            back[f][s] = take;
        }
        prev = cur;
    }
    let mut end = 0;
    let mut best = f32::NEG_INFINITY;
    for s in 0..s_len {
        let v = prev[s] + s as f32 * 1e-4;
        if v > best {
            best = v;
            end = s;
        }
    }
    let mut states = vec![0usize; f_len];
    let mut s = end;
    for f in (0..f_len).rev() {
        states[f] = s;
        if f == 0 {
            break;
        }
        if back[f][s] {
            s -= 1;
        }
    }
    (0..w_len)
        .map(|w| {
            let st = 2 * w + 1;
            let first = states.iter().position(|&x| x == st)?;
            let last = states.iter().rposition(|&x| x == st)?;
            Some((first as f64 * FRAME_DURATION, last as f64 * FRAME_DURATION))
        })
        .collect()
}

/// Partage les petits trous (≤ max_gap) entre deux mots voisins.
pub fn split_interword_gaps(words: &mut [Timed], max_gap: f64) {
    if max_gap <= 0.0 {
        return;
    }
    for i in 1..words.len() {
        let gap = words[i].start - words[i - 1].end;
        if gap > 0.0 && gap <= max_gap {
            let mid = words[i - 1].end + gap / 2.0;
            words[i - 1].end = mid;
            words[i].start = mid;
        }
    }
}

/// Le début d'un mot ne passe jamais avant la fin du précédent.
pub fn monotonize(words: &mut [Timed]) {
    for j in 1..words.len() {
        let prev_end = words[j - 1].end;
        if words[j].start < prev_end {
            words[j].start = prev_end;
            words[j].end = words[j].end.max(prev_end);
        }
    }
}

/// Mots horodatés (secondes, relatives à la fenêtre), équivalent de `extract_word_timings`.
/// `word_texts` : texte décodé de chaque groupe ; `attn` : [jetons générés][positions encodeur].
pub fn extract(words: &[Vec<usize>], word_texts: &[String], attn: &[Vec<f32>], mel: &[Vec<f32>]) -> Vec<Option<Timed>> {
    if words.is_empty() || attn.is_empty() {
        return vec![];
    }
    let f_len = attn[0].len();
    let tok = token_logp(attn, SHARPEN);
    let blank = blank_logp_from_mel(mel, f_len, BLANK_GAMMA, BLANK_PENALTY);
    let spans = viterbi_words(&tok, &blank, words);
    let mut placed: Vec<Timed> = Vec::new();
    let mut idx = Vec::new();
    for (i, (t, span)) in word_texts.iter().zip(&spans).enumerate() {
        if let Some((s, e)) = span {
            placed.push(Timed { word: t.clone(), start: *s, end: *e });
            idx.push(i);
        }
    }
    split_interword_gaps(&mut placed, SPLIT_GAP_MAX);
    let mut out: Vec<Option<Timed>> = vec![None; word_texts.len()];
    for (t, i) in placed.into_iter().zip(idx) {
        out[i] = Some(t);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_on_space_token() {
        let ids = [50258, 1000, 220, 2000, 2001, 220, 3000];
        let p: Vec<String> = ["<|startoftranscript|>", "Bon", " ", "jou", "r", " ", "[UH]"].iter().map(|s| s.to_string()).collect();
        assert_eq!(group_tokens_into_words(&ids, &p), vec![vec![1], vec![3, 4], vec![6]]);
    }

    #[test]
    fn viterbi_places_two_words() {
        // 2 mots sur 10 positions : le premier regarde 2-3, le second 6-7 ; silence ailleurs.
        let f = 10;
        let row = |a: usize, b: usize| (0..f).map(|i| if i >= a && i <= b { 0.5 } else { 0.0001 }).collect::<Vec<f32>>();
        let attn = vec![row(2, 3), row(6, 7)];
        let tok = token_logp(&attn, 5.0);
        let blank: Vec<f32> = (0..f).map(|i| if (2..=3).contains(&i) || (6..=7).contains(&i) { -9.0 } else { -0.01 }).collect();
        let r = viterbi_words(&tok, &blank, &[vec![0], vec![1]]);
        assert_eq!(r[0], Some((0.04, 0.06)));
        assert_eq!(r[1], Some((0.12, 0.14)));
    }
}
