//! Opérations sur des listes d'intervalles (début, fin) en secondes.

pub type Interval = (f64, f64);

/// Fusionne les intervalles qui se chevauchent ou sont séparés de moins de `gap`.
pub fn merge_gap(items: &[Interval], gap: f64) -> Vec<Interval> {
    let mut v: Vec<Interval> = items.iter().copied().filter(|i| i.1 > i.0).collect();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mut out: Vec<Interval> = Vec::with_capacity(v.len());
    for (s, e) in v {
        if let Some(last) = out.last_mut() {
            if s - last.1 <= gap {
                last.1 = last.1.max(e);
                continue;
            }
        }
        out.push((s, e));
    }
    out
}

pub fn merge(items: &[Interval]) -> Vec<Interval> {
    merge_gap(items, 0.0)
}

pub fn pad(items: &[Interval], amount: f64, lo: f64, hi: f64) -> Vec<Interval> {
    let v: Vec<Interval> = items.iter().map(|&(s, e)| ((s - amount).max(lo), (e + amount).min(hi))).collect();
    merge(&v)
}

/// Retire `cuts` de `items`.
pub fn subtract(items: &[Interval], cuts: &[Interval]) -> Vec<Interval> {
    let cuts = merge(cuts);
    let mut out = Vec::new();
    for (s, e) in merge(items) {
        let mut cur = s;
        for &(cs, ce) in &cuts {
            if ce <= cur || cs >= e {
                continue;
            }
            if cs > cur {
                out.push((cur, cs));
            }
            cur = cur.max(ce);
            if cur >= e {
                break;
            }
        }
        if cur < e {
            out.push((cur, e));
        }
    }
    out
}

pub fn invert(items: &[Interval], lo: f64, hi: f64) -> Vec<Interval> {
    subtract(&[(lo, hi)], items)
}

pub fn total(items: &[Interval]) -> f64 {
    items.iter().map(|(s, e)| e - s).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subtract_and_invert() {
        assert_eq!(subtract(&[(0.0, 10.0)], &[(2.0, 3.0), (5.0, 6.0)]), vec![(0.0, 2.0), (3.0, 5.0), (6.0, 10.0)]);
        assert_eq!(invert(&[(1.0, 2.0)], 0.0, 3.0), vec![(0.0, 1.0), (2.0, 3.0)]);
        assert_eq!(merge(&[(0.0, 1.0), (1.0, 2.0), (3.0, 4.0)]), vec![(0.0, 2.0), (3.0, 4.0)]);
    }
}
