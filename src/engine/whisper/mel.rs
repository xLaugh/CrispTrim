//! Spectrogramme log-mel de Whisper, port de `crisperwhisper/features.py`
//! (FeatureExtractor de faster-whisper 1.2.1) : Hann 400, pas 160, filtres « slaney ».

use rustfft::{num_complex::Complex, FftPlanner};

pub const SAMPLE_RATE: usize = 16000;
pub const N_FFT: usize = 400;
pub const HOP: usize = 160;
pub const N_SAMPLES: usize = 30 * SAMPLE_RATE;
pub const N_FRAMES: usize = N_SAMPLES / HOP; // 3000

/// Filtres mel (slaney), [n_mels][N_FFT/2 + 1], comme `get_mel_filters`.
pub fn mel_filters(n_mels: usize) -> Vec<Vec<f64>> {
    let n_freq = N_FFT / 2 + 1;
    let fftfreqs: Vec<f64> = (0..n_freq).map(|i| i as f64 * SAMPLE_RATE as f64 / N_FFT as f64).collect();
    let (min_mel, max_mel) = (0.0f64, 45.245640471924965f64);
    let mels: Vec<f64> = (0..n_mels + 2).map(|i| min_mel + (max_mel - min_mel) * i as f64 / (n_mels + 1) as f64).collect();
    let f_sp = 200.0 / 3.0;
    let min_log_hz = 1000.0;
    let min_log_mel = min_log_hz / f_sp;
    let logstep = 6.4f64.ln() / 27.0;
    let freqs: Vec<f64> = mels
        .iter()
        .map(|&m| if m >= min_log_mel { min_log_hz * (logstep * (m - min_log_mel)).exp() } else { f_sp * m })
        .collect();
    let fdiff: Vec<f64> = freqs.windows(2).map(|w| w[1] - w[0]).collect();
    (0..n_mels)
        .map(|i| {
            let enorm = 2.0 / (freqs[i + 2] - freqs[i]);
            fftfreqs
                .iter()
                .map(|&f| {
                    let lower = -(freqs[i] - f) / fdiff[i];
                    let upper = (freqs[i + 2] - f) / fdiff[i + 1];
                    lower.min(upper).max(0.0) * enorm
                })
                .collect()
        })
        .collect()
}

/// log-mel [n_mels][frames] d'un signal 16 kHz (déjà découpé/complété par l'appelant).
/// `padding` : zéros ajoutés à la fin (160 par défaut dans faster-whisper).
pub fn log_mel(audio: &[f32], n_mels: usize, padding: usize) -> Vec<Vec<f32>> {
    let filters = mel_filters(n_mels);
    let mut x: Vec<f32> = audio.to_vec();
    x.extend(std::iter::repeat(0.0).take(padding));
    // centrage par réflexion (np.pad mode="reflect")
    let pad = N_FFT / 2;
    let n = x.len();
    let mut padded = Vec::with_capacity(n + 2 * pad);
    for i in (1..=pad).rev() {
        padded.push(x[i.min(n - 1)]);
    }
    padded.extend_from_slice(&x);
    for i in 0..pad {
        padded.push(x[n.saturating_sub(2 + i)]);
    }
    let n_frames = 1 + (padded.len() - N_FFT) / HOP;
    // np.hanning(N_FFT + 1)[:-1]
    let window: Vec<f32> =
        (0..N_FFT).map(|i| (0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / N_FFT as f64).cos()) as f32).collect();
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(N_FFT);
    let n_freq = N_FFT / 2 + 1;
    // magnitudes² de toutes les trames sauf la dernière ([..., :-1])
    let used = n_frames - 1;
    let mut mel = vec![vec![0f32; used]; n_mels];
    let mut buf = vec![Complex::new(0f32, 0f32); N_FFT];
    let mut mag = vec![0f64; n_freq];
    for t in 0..used {
        for i in 0..N_FFT {
            buf[i] = Complex::new(padded[t * HOP + i] * window[i], 0.0);
        }
        fft.process(&mut buf);
        for k in 0..n_freq {
            // complex64 comme numpy : norme en float32 puis carré
            let a = buf[k].norm();
            mag[k] = (a * a) as f64;
        }
        for (m, row) in filters.iter().enumerate() {
            let s: f64 = row.iter().zip(&mag).map(|(w, v)| w * v).sum();
            mel[m][t] = s as f32;
        }
    }
    let mut max = f32::NEG_INFINITY;
    for row in mel.iter_mut() {
        for v in row.iter_mut() {
            *v = v.max(1e-10).log10();
            max = max.max(*v);
        }
    }
    for row in mel.iter_mut() {
        for v in row.iter_mut() {
            *v = (v.max(max - 8.0) + 4.0) / 4.0;
        }
    }
    mel
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mel_shape_and_range() {
        let audio: Vec<f32> = (0..16000).map(|i| (i as f32 * 0.05).sin() * 0.3).collect();
        let m = log_mel(&audio, 80, 160);
        assert_eq!(m.len(), 80);
        assert_eq!(m[0].len(), 101); // (16000 + 160) / 160 trames, la dernière retirée
        let max = m.iter().flatten().cloned().fold(f32::MIN, f32::max);
        assert!((max - 1.0).abs() < 1.0);
    }
}
