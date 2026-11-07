//! Timeline : forme d'onde, silences hachurés, hésitations en orange, règle et tête de lecture.

use super::jobs::Cut;
use super::theme::{self, font, rgba, stroke};
use crate::engine::cutter::Kind;
use crate::engine::silence::FRAME;
use eframe::egui::{self, Align2, Color32, CornerRadius, Pos2, Rect, Shape, Stroke, StrokeKind, Vec2};

pub const RULER: f32 = 20.0;
const FLOOR: f32 = 70.0;
pub const MAX_PPS: f64 = 600.0;

pub struct Timeline {
    /// Pixels par seconde.
    pub pps: f64,
    pub start: f64,
    pub fitted: Option<u64>,
    pub width: f32,
    pub dragging: bool,
}

impl Default for Timeline {
    fn default() -> Self {
        Timeline { pps: 50.0, start: 0.0, fitted: None, width: 800.0, dragging: false }
    }
}

impl Timeline {
    pub fn view_dur(&self) -> f64 {
        self.width as f64 / self.pps
    }

    pub fn fit(&mut self, duration: f64, id: u64) {
        self.pps = ((self.width as f64 - 2.0) / duration.max(1e-6)).max(1e-3);
        self.start = 0.0;
        self.fitted = Some(id);
    }

    pub fn clamp(&mut self, duration: f64) {
        let min_pps = (self.width as f64 - 2.0) / duration.max(0.1);
        self.pps = self.pps.max(min_pps).min(MAX_PPS);
        self.start = self.start.max(0.0).min((duration - self.view_dur()).max(0.0));
    }

    pub fn zoom(&mut self, factor: f64, anchor_x: f32, duration: f64) {
        let t = self.start + anchor_x as f64 / self.pps;
        self.pps *= factor;
        self.clamp(duration);
        self.start = t - anchor_x as f64 / self.pps;
        self.clamp(duration);
    }

    pub fn center_on(&mut self, t: f64, duration: f64) {
        let vd = self.view_dur();
        if t < self.start || t > self.start + vd {
            self.start = t - vd * 0.3;
            self.clamp(duration);
        }
    }

    pub fn x_to_t(&self, x: f32) -> f64 {
        self.start + x as f64 / self.pps
    }
}

/// Format d'un temps : 1:05.3 (précis) ou 1:05.
pub fn fmt(t: f64, precise: bool) -> String {
    let t = if t.is_finite() { t } else { 0.0 };
    let neg = t < 0.0;
    let t = t.abs();
    let h = (t / 3600.0).floor() as i64;
    let m = ((t % 3600.0) / 60.0).floor() as i64;
    let s = t % 60.0;
    let ss = if precise { format!("{:04.1}", s) } else { format!("{:02}", s.floor() as i64) };
    let head = if h > 0 { format!("{h}:{m:02}:") } else { format!("{m}:") };
    format!("{}{head}{ss}", if neg { "-" } else { "" })
}

pub struct DrawArgs<'a> {
    pub duration: f64,
    pub cuts: &'a [Cut],
    pub db: &'a [f32],
    pub threshold: f64,
    pub playhead: f64,
}

pub fn draw(painter: &egui::Painter, rect: Rect, tl: &Timeline, a: &DrawArgs) {
    let p = painter.with_clip_rect(rect);
    let w = rect.width();
    let h = rect.height();
    p.rect_filled(rect, CornerRadius::ZERO, theme::PANEL);
    let t0 = tl.start;
    let t1 = tl.start + w as f64 / tl.pps;
    let x = |t: f64| rect.left() + ((t - t0) * tl.pps) as f32;
    let body_top = rect.top() + RULER;
    let body_h = h - RULER;
    let dur = a.duration;

    // régions
    let x0 = x(0.0).max(rect.left());
    let x1 = x(dur).min(rect.right());
    if x1 > x0 {
        p.rect_filled(Rect::from_min_max(Pos2::new(x0, body_top), Pos2::new(x1, rect.bottom())), CornerRadius::ZERO, Color32::from_rgb(0x1f, 0x26, 0x36));
    }
    for c in a.cuts {
        if c.end < t0 || c.start > t1 {
            continue;
        }
        let cx = x(c.start);
        let cw = (x(c.end) - cx).max(1.0);
        let r = Rect::from_min_size(Pos2::new(cx, body_top), Vec2::new(cw, body_h));
        if !c.on {
            let col = if c.kind == Kind::Filler { rgba(255, 157, 66, 0.6) } else { rgba(160, 168, 185, 0.45) };
            let rr = r.shrink(0.5);
            let pts = vec![rr.left_top(), rr.right_top(), rr.right_bottom(), rr.left_bottom(), rr.left_top()];
            p.extend(Shape::dashed_line(&pts, stroke(1.0, col), 4.0, 3.0));
            continue;
        }
        if c.kind == Kind::Filler {
            p.rect_filled(r, CornerRadius::ZERO, rgba(255, 157, 66, 0.22));
            p.rect_filled(Rect::from_min_size(r.min, Vec2::new(cw, 3.0)), CornerRadius::ZERO, theme::FILLER);
        } else {
            p.rect_filled(r, CornerRadius::ZERO, theme::PANEL);
            theme::hatch(&p, r, rgba(140, 148, 165, 0.22));
        }
    }

    // forme d'onde
    if !a.db.is_empty() {
        let mid = body_top + body_h / 2.0;
        let amp = body_h / 2.0 - 6.0;
        let mut ci = 0;
        let n = a.db.len();
        let mut px = 0.0f32;
        while px < w {
            let ta = t0 + px as f64 / tl.pps;
            let tb = t0 + (px + 1.0) as f64 / tl.pps;
            if ta > dur {
                break;
            }
            let i0 = (ta / FRAME).floor().max(0.0) as usize;
            let i1 = ((tb / FRAME).floor() as usize).max(i0 + 1).min(n);
            // Même lecture que l'original : -dB arrondi en octet.
            let mut best = 255.0f32;
            for i in i0..i1 {
                let v = (-a.db[i]).clamp(0.0, 255.0).floor();
                best = best.min(v);
            }
            let amp_n = ((FLOOR - best) / FLOOR).clamp(0.0, 1.0);
            let bh = (amp_n * amp).max(1.0);
            while ci < a.cuts.len() && a.cuts[ci].end < ta {
                ci += 1;
            }
            let in_cut = ci < a.cuts.len() && a.cuts[ci].on && ta >= a.cuts[ci].start && ta <= a.cuts[ci].end;
            let col = if in_cut { Color32::from_rgb(0x4b, 0x51, 0x60) } else { Color32::from_rgb(0xa6, 0xb2, 0xd4) };
            p.rect_filled(Rect::from_min_size(Pos2::new(rect.left() + px, mid - bh), Vec2::new(1.0, bh * 2.0)), CornerRadius::ZERO, col);
            px += 1.0;
        }
        // seuil de silence
        let at = ((FLOOR + a.threshold as f32) / FLOOR).clamp(0.0, 1.0) * amp;
        let col = rgba(143, 123, 255, 0.55);
        for y in [mid - at, mid + at] {
            p.extend(Shape::dashed_line(&[Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)], stroke(1.0, col), 3.0, 4.0));
        }
    }

    // règle
    p.rect_filled(Rect::from_min_size(rect.min, Vec2::new(w, RULER)), CornerRadius::ZERO, Color32::from_rgb(0x0e, 0x11, 0x17));
    let steps = [0.1, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 300.0, 600.0, 1800.0];
    let step = steps.iter().copied().find(|s| s * tl.pps >= 80.0).unwrap_or(3600.0);
    let mut t = (t0 / step).floor() * step;
    while t <= t1 {
        let tx = x(t).round() + 0.5;
        p.line_segment([Pos2::new(tx, rect.top() + RULER - 6.0), Pos2::new(tx, rect.top() + RULER)], stroke(1.0, Color32::from_rgb(0x2a, 0x2f, 0x3a)));
        p.text(Pos2::new(tx + 4.0, rect.top() + 13.0), Align2::LEFT_BOTTOM, fmt(t, step < 1.0), font(11.0), Color32::from_rgb(0x6f, 0x77, 0x87));
        t += step;
    }

    // tête de lecture
    let px = x(a.playhead);
    if px >= rect.left() - 2.0 && px <= rect.right() + 2.0 {
        p.rect_filled(Rect::from_min_size(Pos2::new(px.round() - 1.0, rect.top()), Vec2::new(2.0, h)), CornerRadius::ZERO, Color32::WHITE);
        p.add(Shape::convex_polygon(
            vec![Pos2::new(px - 6.0, rect.top()), Pos2::new(px + 6.0, rect.top()), Pos2::new(px, rect.top() + 8.0)],
            Color32::WHITE,
            Stroke::NONE,
        ));
    }
    painter.rect_stroke(rect, CornerRadius::same(12), stroke(1.0, theme::BORDER), StrokeKind::Inside);
}
