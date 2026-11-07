//! Couleurs, polices et petits composants dessinés à la main (style de l'interface d'origine).

use eframe::egui::{
    self, Align2, Color32, CornerRadius, CursorIcon, FontFamily, FontId, Pos2, Rect, Response, Sense, Shape, Stroke,
    StrokeKind, Ui, Vec2,
};

pub const BG: Color32 = Color32::from_rgb(0x0b, 0x0d, 0x12);
pub const PANEL: Color32 = Color32::from_rgb(0x11, 0x14, 0x1b);
pub const PANEL2: Color32 = Color32::from_rgb(0x18, 0x1c, 0x25);
pub const PANEL3: Color32 = Color32::from_rgb(0x1f, 0x24, 0x30);
pub const BORDER: Color32 = Color32::from_rgb(0x22, 0x27, 0x34);
pub const BORDER2: Color32 = Color32::from_rgb(0x2f, 0x36, 0x46);
pub const TEXT: Color32 = Color32::from_rgb(0xe9, 0xeb, 0xf1);
pub const MUTED: Color32 = Color32::from_rgb(0x8b, 0x93, 0xa3);
pub const FAINT: Color32 = Color32::from_rgb(0x5d, 0x65, 0x75);
pub const ACCENT: Color32 = Color32::from_rgb(0x7c, 0x6c, 0xff);
pub const ACCENT2: Color32 = Color32::from_rgb(0xb0, 0xa5, 0xff);
pub const FILLER: Color32 = Color32::from_rgb(0xff, 0x9d, 0x42);
pub const DANGER: Color32 = Color32::from_rgb(0xff, 0x5d, 0x6c);
pub const OK: Color32 = Color32::from_rgb(0x3d, 0xdc, 0x97);
pub const GRAD: [Color32; 3] =
    [Color32::from_rgb(0x8b, 0x7b, 0xff), Color32::from_rgb(0x6a, 0x5c, 0xff), Color32::from_rgb(0x4f, 0xa8, 0xff)];

/// Trait de contour (épaisseur en pixels).
pub fn stroke(width: f32, color: Color32) -> Stroke {
    Stroke { width, color: color.into() }
}

pub fn accent_soft() -> Color32 {
    rgba(124, 108, 255, 0.14)
}

/// Couleur semi-transparente mélangée comme dans un navigateur (pré-multipliée en sRGB).
pub fn rgba(r: u8, g: u8, b: u8, a: f32) -> Color32 {
    let a = a.clamp(0.0, 1.0);
    let m = |c: u8| (c as f32 * a).round() as u8;
    Color32::from_rgba_premultiplied(m(r), m(g), m(b), (a * 255.0).round() as u8)
}

pub fn bold() -> FontFamily {
    FontFamily::Name("bold".into())
}

pub fn font(size: f32) -> FontId {
    FontId::proportional(size)
}

pub fn bfont(size: f32) -> FontId {
    FontId::new(size, bold())
}

/// Polices Windows (Segoe UI + symboles) si présentes, sinon celles d'egui.
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let dir = std::path::PathBuf::from(std::env::var_os("WINDIR").unwrap_or_else(|| "C:\\Windows".into())).join("Fonts");
    let mut load = |name: &str, file: &str| -> bool {
        match std::fs::read(dir.join(file)) {
            Ok(bytes) => {
                fonts.font_data.insert(name.to_string(), std::sync::Arc::new(egui::FontData::from_owned(bytes)));
                true
            }
            Err(_) => false,
        }
    };
    let regular = load("segoe", "segoeui.ttf");
    let semibold = load("segoe-sb", "seguisb.ttf") || load("segoe-sb", "segoeuib.ttf");
    let symbols = load("segoe-sym", "seguisym.ttf");
    let defaults = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let mut prop = Vec::new();
    if regular {
        prop.push("segoe".to_string());
    }
    if symbols {
        prop.push("segoe-sym".to_string());
    }
    prop.extend(defaults.iter().cloned());
    let mut b = Vec::new();
    if semibold {
        b.push("segoe-sb".to_string());
    }
    b.extend(prop.iter().cloned());
    fonts.families.insert(FontFamily::Proportional, prop);
    fonts.families.insert(bold(), b);
    ctx.set_fonts(fonts);
}

pub fn style(ctx: &egui::Context) {
    ctx.style_mut(|s| {
        let v = &mut s.visuals;
        *v = egui::Visuals::dark();
        v.panel_fill = PANEL;
        v.window_fill = PANEL;
        v.extreme_bg_color = BG;
        v.faint_bg_color = PANEL2;
        v.override_text_color = Some(TEXT);
        v.selection.bg_fill = rgba(124, 108, 255, 0.45);
        v.selection.stroke = stroke(1.0, ACCENT2);
        v.widgets.noninteractive.bg_stroke = stroke(1.0, BORDER);
        for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
            w.bg_fill = PANEL2;
            w.weak_bg_fill = PANEL2;
            w.corner_radius = CornerRadius::same(8);
            w.fg_stroke = stroke(1.0, TEXT);
        }
        v.widgets.inactive.bg_stroke = stroke(1.0, BORDER);
        v.widgets.hovered.bg_stroke = stroke(1.0, BORDER2);
        v.widgets.hovered.weak_bg_fill = PANEL3;
        v.widgets.active.bg_stroke = stroke(1.0, ACCENT);
        v.widgets.open.bg_stroke = stroke(1.0, BORDER2);
        v.text_cursor.stroke = stroke(2.0, ACCENT2);
        s.spacing.item_spacing = Vec2::new(8.0, 6.0);
        s.spacing.scroll.bar_width = 8.0;
        s.text_styles.insert(egui::TextStyle::Body, font(13.5));
        s.text_styles.insert(egui::TextStyle::Button, font(13.5));
        s.text_styles.insert(egui::TextStyle::Small, font(11.5));
    });
}

// --------------------------------------------------------------------------- formes

/// Points d'un rectangle arrondi (sens horaire).
fn rounded_points(rect: Rect, r: f32) -> Vec<Pos2> {
    let r = r.min(rect.width() / 2.0).min(rect.height() / 2.0);
    let mut pts = Vec::new();
    let corners = [
        (Pos2::new(rect.right() - r, rect.top() + r), -90.0f32),
        (Pos2::new(rect.right() - r, rect.bottom() - r), 0.0),
        (Pos2::new(rect.left() + r, rect.bottom() - r), 90.0),
        (Pos2::new(rect.left() + r, rect.top() + r), 180.0),
    ];
    for (c, a0) in corners {
        for i in 0..=8 {
            let a = (a0 + 90.0 * i as f32 / 8.0).to_radians();
            pts.push(Pos2::new(c.x + r * a.cos(), c.y + r * a.sin()));
        }
    }
    pts
}

fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_premultiplied(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()), m(a.a(), b.a()))
}

/// Couleur d'un dégradé à plusieurs arrêts (positions 0..1).
fn grad_at(stops: &[(f32, Color32)], t: f32) -> Color32 {
    if t <= stops[0].0 {
        return stops[0].1;
    }
    for w in stops.windows(2) {
        if t <= w[1].0 {
            return lerp(w[0].1, w[1].1, (t - w[0].0) / (w[1].0 - w[0].0));
        }
    }
    stops[stops.len() - 1].1
}

/// Rectangle arrondi rempli d'un dégradé diagonal (135°).
pub fn gradient_rect(painter: &egui::Painter, rect: Rect, radius: f32, stops: &[(f32, Color32)]) {
    let pts = rounded_points(rect, radius);
    let mut mesh = egui::Mesh::default();
    let span = rect.width() + rect.height();
    let col = |p: Pos2| grad_at(stops, ((p.x - rect.left()) + (p.y - rect.top())) / span);
    mesh.colored_vertex(rect.center(), col(rect.center()));
    for p in &pts {
        mesh.colored_vertex(*p, col(*p));
    }
    let n = pts.len() as u32;
    for i in 0..n {
        mesh.add_triangle(0, 1 + i, 1 + (i + 1) % n);
    }
    painter.add(Shape::mesh(mesh));
}

/// Dégradé principal de l'interface (violet → bleu).
pub fn accent_rect(painter: &egui::Painter, rect: Rect, radius: f32) {
    gradient_rect(painter, rect, radius, &[(0.0, GRAD[0]), (0.55, GRAD[1]), (1.2, GRAD[2])]);
}

pub fn glow(painter: &egui::Painter, rect: Rect, radius: f32, alpha: f32) {
    for i in 1..=4 {
        let e = i as f32 * 2.0;
        painter.rect_filled(rect.expand(e).translate(Vec2::new(0.0, 4.0)), CornerRadius::same((radius + e) as u8), rgba(124, 108, 255, alpha / (i as f32 * 2.0)));
    }
}

pub fn text_width(ui: &Ui, s: &str, f: FontId) -> f32 {
    ui.painter().layout_no_wrap(s.to_string(), f, TEXT).size().x
}

// --------------------------------------------------------------------------- boutons

#[derive(Clone, Copy, PartialEq)]
pub enum Btn {
    Primary,
    Normal,
    Ghost,
}

/// Bouton de l'interface : `label` à gauche, `small` (extension) à droite si `wide`.
pub fn button(ui: &mut Ui, size: Vec2, kind: Btn, label: &str, small: &str, enabled: bool, f: FontId) -> Response {
    let (rect, resp) = ui.allocate_exact_size(size, if enabled { Sense::click() } else { Sense::hover() });
    let p = ui.painter();
    let hovered = enabled && resp.hovered();
    let rect_d = if enabled && resp.is_pointer_button_down_on() { rect.translate(Vec2::new(0.0, 1.0)) } else { rect };
    let radius = if size.y >= 44.0 { 11.0 } else { 9.0 };
    let alpha = if enabled { 1.0 } else { 0.4 };
    let fg = match kind {
        Btn::Primary => Color32::WHITE,
        _ => TEXT,
    }
    .gamma_multiply(alpha);
    match kind {
        Btn::Primary => {
            if enabled {
                glow(p, rect_d, radius, if hovered { 0.4 } else { 0.28 });
            }
            let p2 = p.clone().with_layer_id(p.layer_id());
            if enabled {
                accent_rect(&p2, rect_d, radius);
                if hovered {
                    p2.rect_filled(rect_d, CornerRadius::same(radius as u8), rgba(255, 255, 255, 0.08));
                }
            } else {
                gradient_rect(&p2, rect_d, radius, &[(0.0, GRAD[0].gamma_multiply(0.4)), (0.55, GRAD[1].gamma_multiply(0.4)), (1.2, GRAD[2].gamma_multiply(0.4))]);
            }
        }
        Btn::Normal | Btn::Ghost => {
            let fill = if hovered { PANEL3 } else if kind == Btn::Ghost { Color32::TRANSPARENT } else { PANEL2 };
            let border = if hovered { BORDER2 } else { BORDER };
            p.rect(rect_d, CornerRadius::same(radius as u8), fill.gamma_multiply(alpha), stroke(1.0, border.gamma_multiply(alpha)), StrokeKind::Inside);
        }
    }
    let bold = kind == Btn::Primary;
    let lf = if bold { FontId::new(f.size, crate::ui::theme::bold()) } else { f.clone() };
    if small.is_empty() {
        p.text(rect_d.center(), Align2::CENTER_CENTER, label, lf, fg);
    } else {
        p.text(Pos2::new(rect_d.left() + 14.0, rect_d.center().y), Align2::LEFT_CENTER, label, lf, fg);
        p.text(Pos2::new(rect_d.right() - 12.0, rect_d.center().y), Align2::RIGHT_CENTER, small, font(11.5), fg.gamma_multiply(0.65));
    }
    if enabled {
        resp.on_hover_cursor(CursorIcon::PointingHand)
    } else {
        resp
    }
}

/// Bouton carré à icône.
pub fn icon_button(ui: &mut Ui, size: Vec2, icon: &str, icon_size: f32, primary: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let p = ui.painter();
    let hovered = resp.hovered();
    if primary {
        glow(p, rect, 9.0, 0.3);
        accent_rect(p, rect, 9.0);
        if hovered {
            p.rect_filled(rect, CornerRadius::same(9), rgba(255, 255, 255, 0.08));
        }
    } else {
        p.rect(rect, CornerRadius::same(9), if hovered { PANEL3 } else { PANEL2 }, stroke(1.0, if hovered { BORDER2 } else { BORDER }), StrokeKind::Inside);
    }
    p.text(rect.center(), Align2::CENTER_CENTER, icon, font(icon_size), if primary { Color32::WHITE } else { TEXT });
    resp.on_hover_cursor(CursorIcon::PointingHand)
}

/// Interrupteur (pilule 32×18) suivi d'un libellé ; renvoie vrai si la valeur a changé.
pub fn switch(ui: &mut Ui, on: &mut bool, label: &str, label_color: Color32, f: FontId, wrap_width: f32) -> bool {
    let galley = ui.painter().layout(label.to_string(), f, label_color, wrap_width - 42.0);
    let h = galley.size().y.max(18.0);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(wrap_width.min(42.0 + galley.size().x), h), Sense::click());
    let p = ui.painter();
    let pill = Rect::from_min_size(Pos2::new(rect.left(), rect.top() + (galley.rows.first().map(|r| r.height()).unwrap_or(18.0) - 18.0) / 2.0), Vec2::new(32.0, 18.0));
    p.rect_filled(pill, CornerRadius::same(9), if *on { ACCENT } else { Color32::from_rgb(0x34, 0x3a, 0x47) });
    let cx = if *on { pill.left() + 23.0 } else { pill.left() + 9.0 };
    p.circle_filled(Pos2::new(cx, pill.center().y + 0.5), 7.5, rgba(0, 0, 0, 0.3));
    p.circle_filled(Pos2::new(cx, pill.center().y), 7.0, Color32::WHITE);
    p.galley(Pos2::new(rect.left() + 42.0, rect.top()), galley, label_color);
    let changed = resp.clicked();
    if changed {
        *on = !*on;
    }
    resp.on_hover_cursor(CursorIcon::PointingHand);
    changed
}

/// Petite case à cocher violette.
pub fn checkbox(ui: &mut Ui, on: &mut bool, label: &str) -> bool {
    let w = if label.is_empty() { 16.0 } else { 20.0 + text_width(ui, label, font(11.5)) };
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 16.0), Sense::click());
    let p = ui.painter();
    let b = Rect::from_min_size(Pos2::new(rect.left(), rect.center().y - 7.0), Vec2::splat(14.0));
    if *on {
        p.rect_filled(b, CornerRadius::same(3), ACCENT);
        p.line_segment([b.left_center() + Vec2::new(3.0, 0.0), Pos2::new(b.left() + 6.0, b.bottom() - 3.5)], stroke(1.8, Color32::WHITE));
        p.line_segment([Pos2::new(b.left() + 6.0, b.bottom() - 3.5), Pos2::new(b.right() - 3.0, b.top() + 3.5)], stroke(1.8, Color32::WHITE));
    } else {
        p.rect(b, CornerRadius::same(3), PANEL, stroke(1.0, MUTED), StrokeKind::Inside);
    }
    if !label.is_empty() {
        p.text(Pos2::new(b.right() + 5.0, rect.center().y), Align2::LEFT_CENTER, label, font(11.5), MUTED);
    }
    let c = resp.clicked();
    if c {
        *on = !*on;
    }
    resp.on_hover_cursor(CursorIcon::PointingHand);
    c
}

/// Curseur : piste 4 px remplie en violet, poignée blanche cerclée.
pub fn slider(ui: &mut Ui, value: &mut f64, min: f64, max: f64, step: f64, enabled: bool) -> bool {
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 18.0), if enabled { Sense::click_and_drag() } else { Sense::hover() });
    let track = Rect::from_center_size(rect.center(), Vec2::new(w - 16.0, 4.0));
    let mut changed = false;
    if enabled && (resp.dragged() || resp.clicked()) {
        if let Some(pos) = resp.interact_pointer_pos() {
            let t = ((pos.x - track.left()) / track.width()).clamp(0.0, 1.0) as f64;
            let mut v = min + t * (max - min);
            v = (v / step).round() * step;
            v = v.clamp(min, max);
            if (v - *value).abs() > 1e-9 {
                *value = v;
                changed = true;
            }
        }
    }
    let alpha = if enabled { 1.0 } else { 0.4 };
    let p = ui.painter();
    let t = (((*value - min) / (max - min)).clamp(0.0, 1.0)) as f32;
    let x = track.left() + t * track.width();
    p.rect_filled(track, CornerRadius::same(2), PANEL3.gamma_multiply(alpha));
    p.rect_filled(Rect::from_min_max(track.min, Pos2::new(x, track.max.y)), CornerRadius::same(2), ACCENT.gamma_multiply(alpha));
    let c = Pos2::new(x, track.center().y);
    p.circle_filled(c + Vec2::new(0.0, 1.0), 8.5, rgba(0, 0, 0, 0.3 * alpha));
    p.circle_filled(c, 8.0, ACCENT.gamma_multiply(alpha));
    p.circle_filled(c, 5.0, Color32::WHITE.gamma_multiply(alpha));
    if enabled {
        resp.on_hover_cursor(CursorIcon::PointingHand);
    }
    changed
}

/// Pastille de valeur (fond violet pâle).
pub fn pill(ui: &Ui, right: Pos2, s: &str) {
    let p = ui.painter();
    let w = text_width(ui, s, font(12.0)).max(46.0) + 12.0;
    let r = Rect::from_min_size(Pos2::new(right.x - w, right.y - 10.0), Vec2::new(w, 20.0));
    p.rect_filled(r, CornerRadius::same(6), accent_soft());
    p.text(r.center(), Align2::CENTER_CENTER, s, font(12.0), ACCENT2);
}

/// Logo : carré dégradé avec des ciseaux.
pub fn logo(painter: &egui::Painter, rect: Rect, radius: f32) {
    glow(painter, rect, radius, 0.35);
    accent_rect(painter, rect, radius);
    painter.text(rect.center(), Align2::CENTER_CENTER, "✂", font(rect.height() * 0.5), Color32::WHITE);
}

/// Hachures diagonales (zones de silence).
pub fn hatch(painter: &egui::Painter, rect: Rect, color: Color32) {
    let p = painter.with_clip_rect(rect.intersect(painter.clip_rect()));
    let mut x = rect.left() - rect.height();
    let start = ((x / 8.0).floor()) * 8.0;
    x = start;
    while x < rect.right() + 8.0 {
        p.line_segment([Pos2::new(x, rect.bottom()), Pos2::new(x + rect.height(), rect.top())], stroke(1.4, color));
        x += 8.0;
    }
}
