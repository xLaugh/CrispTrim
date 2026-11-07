//! Moteur de CrispTrim : analyse, coupes, exports. Sans interface.

pub mod cutter;
pub mod export;
pub mod fillers;
pub mod gpu_worker;
pub mod intervals;
pub mod media;
pub mod silence;
pub mod text;
pub mod transcribe;
pub mod updates;
pub mod whisper;

mod textfile;
pub use textfile::write_text;

/// Cadence d'images exacte (fraction réduite), comme `fractions.Fraction`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fps {
    pub num: i64,
    pub den: i64,
}

fn gcd(a: i64, b: i64) -> i64 {
    if b == 0 { a.abs() } else { gcd(b, a % b) }
}

/// Cadences standard : les vidéos de téléphone (cadence variable) sont ramenées à la plus proche.
const STANDARD_FPS: [(i64, i64); 12] = [
    (24000, 1001), (24, 1), (25, 1), (30000, 1001), (30, 1), (48, 1),
    (50, 1), (60000, 1001), (60, 1), (100, 1), (120000, 1001), (120, 1),
];

impl Fps {
    pub fn new(num: i64, den: i64) -> Self {
        let g = gcd(num, den).max(1);
        let s = if den < 0 { -1 } else { 1 };
        Fps { num: s * num / g, den: s * den / g }
    }

    pub fn parse(s: &str) -> Option<Fps> {
        let (a, b) = s.split_once('/').unwrap_or((s, "1"));
        let (a, b): (i64, i64) = (a.trim().parse().ok()?, b.trim().parse().ok()?);
        (a > 0 && b > 0).then(|| Fps::new(a, b))
    }

    pub fn as_f64(self) -> f64 {
        self.num as f64 / self.den as f64
    }

    pub fn snap(self) -> Fps {
        let f = self.as_f64();
        let best = STANDARD_FPS
            .iter()
            .map(|&(n, d)| Fps::new(n, d))
            .min_by(|a, b| (a.as_f64() - f).abs().partial_cmp(&(b.as_f64() - f).abs()).unwrap())
            .unwrap();
        if (best.as_f64() - f).abs() / best.as_f64() < 0.01 { best } else { self }
    }

    /// Image → secondes, exact.
    pub fn secs(self, frames: i64) -> f64 {
        frames as f64 * self.den as f64 / self.num as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fps_snap() {
        assert_eq!(Fps::parse("2997/100").unwrap().snap(), Fps::new(30000, 1001));
        assert_eq!(Fps::parse("30000/1001").unwrap(), Fps::new(30000, 1001));
        assert_eq!(Fps::parse("12/1").unwrap().snap(), Fps::new(12, 1));
    }
}
