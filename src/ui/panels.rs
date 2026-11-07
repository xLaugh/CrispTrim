//! Barre du haut, liste des rushes, centre (lecteur + timeline).

use super::jobs::Stage;
use super::theme::{self, bfont, font, rgba, stroke, Btn, ACCENT, ACCENT2, BORDER, BORDER2, FAINT, FILLER, MUTED, PANEL, PANEL2, PANEL3, TEXT};
use super::timeline::{self, fmt};
use super::{App, Modal};
use crate::engine::transcribe::MODELS;
use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, CursorIcon, Frame, Layout, Margin, Pos2, Rect, RichText, ScrollArea, Sense,
    Shape, StrokeKind, Ui, UiBuilder, Vec2,
};

pub fn hline(ui: &Ui, y: f32, x0: f32, x1: f32) {
    ui.painter().line_segment([Pos2::new(x0, y), Pos2::new(x1, y)], stroke(1.0, BORDER));
}

pub fn dashed_rect(p: &egui::Painter, r: Rect, color: Color32, width: f32) {
    let pts = vec![r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
    p.extend(Shape::dashed_line(&pts, stroke(width, color), 5.0, 4.0));
}

pub fn in_rect(ui: &mut Ui, rect: Rect, add: impl FnOnce(&mut Ui)) {
    ui.scope_builder(UiBuilder::new().max_rect(rect).layout(Layout::top_down(Align::Min)), add);
}

/// Coupe un texte trop long avec « … ».
pub fn elide(ui: &Ui, s: &str, f: egui::FontId, width: f32) -> String {
    if theme::text_width(ui, s, f.clone()) <= width {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    let mut n = chars.len();
    while n > 1 {
        n -= 1;
        let t: String = chars[..n].iter().collect::<String>() + "…";
        if theme::text_width(ui, &t, f.clone()) <= width {
            return t;
        }
    }
    "…".into()
}

fn pulse(ui: &Ui) -> f32 {
    let t = ui.input(|i| i.time);
    0.3 + 0.7 * (0.5 + 0.5 * (t * std::f64::consts::TAU).cos()) as f32
}

// =========================================================================== barre du haut

pub fn top_bar(app: &mut App, ctx: &egui::Context) {
    egui::TopBottomPanel::top("top")
        .exact_height(56.0)
        .frame(Frame::new().fill(PANEL).inner_margin(Margin::symmetric(16, 0)))
        .show_separator_line(false)
        .show(ctx, |ui| {
            let full = ui.max_rect();
            hline(ui, full.bottom() - 0.5, full.left() - 16.0, full.right() + 16.0);
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                // marque
                let (r, resp) = ui.allocate_exact_size(Vec2::new(112.0, 34.0), Sense::click());
                let logo = Rect::from_min_size(Pos2::new(r.left(), r.center().y - 15.0), Vec2::splat(30.0));
                theme::logo(ui.painter(), logo, 9.0);
                let g = ui.painter().text(Pos2::new(logo.right() + 10.0, r.center().y), Align2::LEFT_CENTER, "Crisp", font(16.0), TEXT);
                ui.painter().text(Pos2::new(g.right(), r.center().y), Align2::LEFT_CENTER, "Trim", bfont(16.0), ACCENT2);
                if resp.on_hover_cursor(CursorIcon::PointingHand).on_hover_text("À propos de CrispTrim").clicked() {
                    app.modal = Modal::About;
                }
                ui.add_space(-6.0);
                let label = "+  Ajouter des rushes";
                let w = theme::text_width(ui, label, bfont(13.5)) + 30.0;
                if theme::button(ui, Vec2::new(w, 34.0), Btn::Primary, label, "", true, font(13.5)).clicked() {
                    app.pick_files();
                }
                // champ « chemin »
                let pw = (ui.available_width() - 330.0).clamp(160.0, 560.0);
                let (pr, _) = ui.allocate_exact_size(Vec2::new(pw, 34.0), Sense::hover());
                let id = egui::Id::new("path-input");
                let focused = ui.memory(|m| m.has_focus(id));
                ui.painter().rect(pr, CornerRadius::same(9), theme::BG, stroke(1.0, if focused { ACCENT } else { BORDER }), StrokeKind::Inside);
                ui.painter().text(Pos2::new(pr.left() + 13.0, pr.center().y), Align2::LEFT_CENTER, "⌕", font(15.0), FAINT);
                let inner = Rect::from_min_max(Pos2::new(pr.left() + 32.0, pr.top() + 8.0), Pos2::new(pr.right() - 10.0, pr.bottom() - 8.0));
                let te = egui::TextEdit::singleline(&mut app.path_input)
                    .id(id)
                    .hint_text(RichText::new("Coller le chemin d'un fichier…").color(FAINT))
                    .frame(false)
                    .desired_width(inner.width());
                let resp = ui.put(inner, te);
                if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    let v = app.path_input.trim().to_string();
                    if !v.is_empty() {
                        app.path_input.clear();
                        app.add_paths(vec![v.into()]);
                    }
                }

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    if theme::icon_button(ui, Vec2::splat(34.0), "ⓘ", 15.0, false).on_hover_text("À propos et crédits").clicked() {
                        app.modal = Modal::About;
                    }
                    let pill = app.update.as_ref().filter(|u| u.available && u.latest.as_deref() != app.skip_version.as_deref()).and_then(|u| u.latest.clone());
                    if let Some(tag) = pill {
                        let label = "Nouvelle version disponible ";
                        let w = theme::text_width(ui, label, font(12.5)) + theme::text_width(ui, &tag, bfont(12.5)) + 40.0;
                        let (r, resp) = ui.allocate_exact_size(Vec2::new(w, 32.0), Sense::click());
                        let hov = resp.hovered();
                        let p = ui.painter();
                        p.rect(r, CornerRadius::same(16), rgba(61, 220, 151, if hov { 0.2 } else { 0.12 }), stroke(1.0, rgba(61, 220, 151, 0.45)), StrokeKind::Inside);
                        p.circle_filled(Pos2::new(r.left() + 16.0, r.center().y), 4.0, theme::OK.gamma_multiply(pulse(ui)));
                        let g = p.text(Pos2::new(r.left() + 28.0, r.center().y), Align2::LEFT_CENTER, label, font(12.5), Color32::from_rgb(0x9f, 0xf0, 0xc8));
                        p.text(Pos2::new(g.right(), r.center().y), Align2::LEFT_CENTER, &tag, bfont(12.5), Color32::WHITE);
                        if resp.on_hover_cursor(CursorIcon::PointingHand).on_hover_text("Voir la nouvelle version").clicked() {
                            app.modal = Modal::Update;
                        }
                    }
                    let current = MODELS.iter().find(|m| m.0 == app.model).map(|m| m.1).unwrap_or("large · précis");
                    let mut chosen = None;
                    egui::ComboBox::from_id_salt("model")
                        .selected_text(RichText::new(current).size(12.5))
                        .width(110.0)
                        .show_ui(ui, |ui| {
                            for (id, label) in MODELS {
                                if ui.selectable_label(app.model == id, label).clicked() {
                                    chosen = Some(id);
                                }
                            }
                        })
                        .response
                        .on_hover_text("Modèle de transcription CrisperWhisper");
                    if let Some(m) = chosen.filter(|m| *m != app.model) {
                        app.set_model(m);
                    }
                    ui.label(RichText::new("Modèle").size(12.0).color(MUTED));
                });
            });
        });
}

// =========================================================================== rushes

struct FileRow {
    id: u64,
    name: String,
    path: String,
    stage: Stage,
    duration: Option<f64>,
    gain: Option<f64>,
    audio: bool,
}

fn fmt_pct(v: f64) -> String {
    if v.fract() == 0.0 {
        format!("{}", v as i64)
    } else {
        format!("{v:.1}")
    }
}

pub fn files_panel(app: &mut App, ctx: &egui::Context) {
    let wide = ctx.screen_rect().width() > 1100.0;
    egui::SidePanel::left("files")
        .exact_width(if wide { 264.0 } else { 220.0 })
        .resizable(false)
        .show_separator_line(false)
        .frame(Frame::new().fill(PANEL))
        .show(ctx, |ui| {
            let full = ui.max_rect();
            ui.painter().line_segment([Pos2::new(full.right() - 0.5, full.top()), Pos2::new(full.right() - 0.5, full.bottom())], stroke(1.0, BORDER));
            let mut rows = Vec::new();
            for j in &app.jobs {
                let j = j.lock().unwrap();
                if let (Some((w, h, px)), false) = (&j.thumb, app.thumbs.contains_key(&j.id)) {
                    let img = egui::ColorImage::from_rgba_unmultiplied([*w as usize, *h as usize], px);
                    app.thumbs.insert(j.id, ctx.load_texture(format!("thumb-{}", j.id), img, egui::TextureOptions::LINEAR));
                }
                rows.push(FileRow {
                    id: j.id,
                    name: j.name.clone(),
                    path: j.path.display().to_string(),
                    stage: j.stage,
                    duration: j.info.as_ref().map(|i| i.duration),
                    gain: (!j.stage.busy() && j.stats.duration > 0.0).then_some(j.stats.removed_pct),
                    audio: j.kind_is_audio(),
                });
            }
            // titre
            let (tr, _) = ui.allocate_exact_size(Vec2::new(full.width(), 40.0), Sense::hover());
            let g = ui.painter().text(Pos2::new(tr.left() + 16.0, tr.top() + 24.0), Align2::LEFT_CENTER, "R U S H E S", bfont(10.5), MUTED);
            if !rows.is_empty() {
                let n = rows.len().to_string();
                let pr = Rect::from_min_size(Pos2::new(g.right() + 8.0, tr.top() + 16.0), Vec2::new(theme::text_width(ui, &n, font(10.5)) + 14.0, 16.0));
                ui.painter().rect_filled(pr, CornerRadius::same(8), PANEL3);
                ui.painter().text(pr.center(), Align2::CENTER_CENTER, n, font(10.5), MUTED);
            }
            if rows.is_empty() {
                let r = Rect::from_min_size(Pos2::new(full.left() + 12.0, tr.bottom() + 4.0), Vec2::new(full.width() - 24.0, 84.0));
                dashed_rect(ui.painter(), r, BORDER2, 1.5);
                ui.painter().text(Pos2::new(r.center().x, r.top() + 30.0), Align2::CENTER_CENTER, "⇣", font(20.0), ACCENT2);
                ui.painter().text(Pos2::new(r.center().x, r.top() + 58.0), Align2::CENTER_CENTER, "Glisse tes vidéos ici", font(12.5), MUTED);
                return;
            }
            let mut select = None;
            let mut remove = None;
            ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                for row in &rows {
                    file_row(app, ui, row, full.width() - 20.0, wide, &mut select, &mut remove);
                }
            });
            if let Some(id) = remove {
                app.remove(id);
            } else if let Some(id) = select {
                app.select(id);
            }
        });
}

fn file_row(app: &App, ui: &mut Ui, row: &FileRow, width: f32, wide: bool, select: &mut Option<u64>, remove: &mut Option<u64>) {
    let busy = row.stage.busy();
    let h = if busy { 64.0 } else { 56.0 };
    ui.horizontal(|ui| {
        ui.add_space(10.0);
        let (r, resp) = ui.allocate_exact_size(Vec2::new(width, h), Sense::click());
        let active = app.sel == Some(row.id);
        let p = ui.painter();
        if active || resp.hovered() {
            p.rect(r, CornerRadius::same(10), PANEL2, stroke(1.0, if active { BORDER2 } else { Color32::TRANSPARENT }), StrokeKind::Inside);
        }
        if active {
            p.rect_filled(Rect::from_min_size(r.min + Vec2::new(0.0, 4.0), Vec2::new(3.0, r.height() - 8.0)), CornerRadius::same(2), ACCENT);
        }
        let mut x = r.left() + 7.0;
        if wide {
            let th = Rect::from_min_size(Pos2::new(x, r.top() + 8.0), Vec2::new(64.0, 40.0));
            p.rect(th, CornerRadius::same(7), PANEL3, stroke(1.0, BORDER), StrokeKind::Inside);
            if let Some(tex) = app.thumbs.get(&row.id) {
                let [tw, tht] = tex.size();
                // « cover » : on remplit la vignette en recadrant
                let (ar_t, ar_b) = (tw as f32 / tht as f32, 64.0 / 40.0);
                let uv = if ar_t > ar_b {
                    let f = ar_b / ar_t;
                    Rect::from_min_max(Pos2::new((1.0 - f) / 2.0, 0.0), Pos2::new((1.0 + f) / 2.0, 1.0))
                } else {
                    let f = ar_t / ar_b;
                    Rect::from_min_max(Pos2::new(0.0, (1.0 - f) / 2.0), Pos2::new(1.0, (1.0 + f) / 2.0))
                };
                egui::Image::new((tex.id(), th.size())).uv(uv).corner_radius(7).paint_at(ui, th.shrink(1.0));
            } else {
                p.text(th.center(), Align2::CENTER_CENTER, if row.audio { "♪" } else { "▶" }, font(16.0), ACCENT2);
            }
            x = th.right() + 10.0;
        }
        let p = ui.painter();
        let name = elide(ui, &row.name, bfont(13.5), r.right() - x - 24.0);
        p.text(Pos2::new(x, r.top() + 20.0), Align2::LEFT_CENTER, name, bfont(13.5), TEXT);
        let dot = match row.stage {
            Stage::Error => theme::DANGER,
            s if s.busy() => ACCENT.gamma_multiply(pulse(ui)),
            _ => theme::OK,
        };
        let my = r.top() + 39.0;
        p.circle_filled(Pos2::new(x + 3.5, my), 3.5, dot);
        let stage = if busy || row.stage == Stage::Error { row.stage.label().to_string() } else { row.duration.map(|d| fmt(d, false)).unwrap_or_default() };
        let stage = elide(ui, &stage, font(11.5), r.right() - x - 80.0);
        p.text(Pos2::new(x + 13.0, my), Align2::LEFT_CENTER, stage, font(11.5), MUTED);
        if let Some(g) = row.gain {
            let s = format!("−{} %", fmt_pct(g));
            let gw = theme::text_width(ui, &s, bfont(11.0)) + 12.0;
            let gr = Rect::from_min_size(Pos2::new(r.right() - 8.0 - gw, my - 8.0), Vec2::new(gw, 16.0));
            p.rect_filled(gr, CornerRadius::same(8), theme::accent_soft());
            p.text(gr.center(), Align2::CENTER_CENTER, s, bfont(11.0), ACCENT2);
        }
        if busy {
            let br = Rect::from_min_size(Pos2::new(r.left() + 7.0, r.bottom() - 9.0), Vec2::new(r.width() - 14.0, 3.0));
            p.rect_filled(br, CornerRadius::same(2), PANEL3);
            let t = (ui.input(|i| i.time) % 1.4 / 1.4) as f32;
            let bw = br.width() * 0.4;
            let bx = br.left() - bw + t * (br.width() + bw * 1.6);
            let seg = Rect::from_min_size(Pos2::new(bx, br.top()), Vec2::new(bw, 3.0)).intersect(br);
            if seg.width() > 0.0 {
                theme::accent_rect(p, seg, 1.5);
            }
        }
        // ✕ au survol
        let mut on_x = false;
        if resp.hovered() {
            let xr = Rect::from_min_size(Pos2::new(r.right() - 26.0, r.top() + 6.0), Vec2::splat(20.0));
            on_x = ui.input(|i| i.pointer.hover_pos()).map(|h| xr.contains(h)).unwrap_or(false);
            if on_x {
                p.rect_filled(xr, CornerRadius::same(6), PANEL3);
            }
            p.text(xr.center(), Align2::CENTER_CENTER, "✕", font(11.0), if on_x { TEXT } else { MUTED });
        }
        if resp.clicked() {
            if on_x {
                *remove = Some(row.id);
            } else {
                *select = Some(row.id);
            }
        }
        resp.on_hover_cursor(CursorIcon::PointingHand).on_hover_text(&row.path);
    });
}

// =========================================================================== centre

pub fn center(app: &mut App, ctx: &egui::Context) {
    egui::CentralPanel::default().frame(Frame::new().fill(theme::BG)).show(ctx, |ui| {
        let full = ui.max_rect();
        if app.view.is_none() {
            empty_state(app, ui, full);
            return;
        }
        let r = full.shrink(14.0);
        let show_hint = ctx.screen_rect().width() > 1100.0;
        let hint_h = if show_hint { 18.0 } else { 0.0 };
        let (tl_h, tr_h) = (160.0, 38.0);
        let gaps = if show_hint { 36.0 } else { 24.0 };
        let player_h = (r.height() - tl_h - tr_h - hint_h - gaps).max(160.0);
        let pr = Rect::from_min_size(r.min, Vec2::new(r.width(), player_h));
        let trr = Rect::from_min_size(Pos2::new(r.left(), pr.bottom() + 12.0), Vec2::new(r.width(), tr_h));
        let tlr = Rect::from_min_size(Pos2::new(r.left(), trr.bottom() + 12.0), Vec2::new(r.width(), tl_h));
        let hr = Rect::from_min_size(Pos2::new(r.left(), tlr.bottom() + 12.0), Vec2::new(r.width(), hint_h));
        player_area(app, ui, pr);
        in_rect(ui, trr, |ui| transport(app, ui));
        timeline_area(app, ui, tlr);
        if show_hint {
            hints(ui, hr);
        }
    });
}

fn empty_state(app: &mut App, ui: &mut Ui, full: Rect) {
    let w = (full.width() - 56.0).min(560.0);
    let box_r = Rect::from_center_size(full.center(), Vec2::new(w, 330.0));
    let p = ui.painter();
    // halo violet en haut de la zone
    for i in 0..10 {
        let f = i as f32 / 10.0;
        let rr = Rect::from_center_size(Pos2::new(box_r.center().x, box_r.top()), Vec2::new(w * (1.0 - f * 0.5), 360.0 * (1.0 - f)));
        p.rect_filled(rr.intersect(box_r.shrink(1.0)), CornerRadius::same(20), rgba(124, 108, 255, 0.012));
    }
    // cadre en pointillés arrondi
    let rad = 20.0;
    let mut pts = Vec::new();
    for (c, a0) in [
        (Pos2::new(box_r.right() - rad, box_r.top() + rad), -90.0f32),
        (Pos2::new(box_r.right() - rad, box_r.bottom() - rad), 0.0),
        (Pos2::new(box_r.left() + rad, box_r.bottom() - rad), 90.0),
        (Pos2::new(box_r.left() + rad, box_r.top() + rad), 180.0),
    ] {
        for i in 0..=6 {
            let a = (a0 + 15.0 * i as f32).to_radians();
            pts.push(Pos2::new(c.x + rad * a.cos(), c.y + rad * a.sin()));
        }
    }
    pts.push(pts[0]);
    p.extend(Shape::dashed_line(&pts, stroke(1.5, BORDER2), 6.0, 4.0));
    let icon = Rect::from_center_size(Pos2::new(box_r.center().x, box_r.top() + 44.0 + 34.0), Vec2::splat(68.0));
    theme::logo(p, icon, 20.0);
    p.text(Pos2::new(box_r.center().x, icon.bottom() + 36.0), Align2::CENTER_CENTER, "Coupe tes silences et tes « euh »", bfont(22.0), TEXT);
    p.text(
        Pos2::new(box_r.center().x, icon.bottom() + 70.0),
        Align2::CENTER_CENTER,
        "Glisse tes rushes ici, ou choisis-les. Tout est traité sur ta machine.",
        font(13.5),
        MUTED,
    );
    let label = "+  Ajouter des rushes";
    let bw = theme::text_width(ui, label, bfont(15.0)) + 44.0;
    let br = Rect::from_center_size(Pos2::new(box_r.center().x, icon.bottom() + 124.0), Vec2::new(bw, 44.0));
    let mut clicked = false;
    in_rect(ui, br, |ui| {
        clicked = theme::button(ui, Vec2::new(bw, 44.0), Btn::Primary, label, "", true, font(15.0)).clicked();
    });
    if clicked {
        app.pick_files();
    }
    ui.painter().text(Pos2::new(box_r.center().x, box_r.bottom() - 30.0), Align2::CENTER_CENTER, "MP4 · MOV · MKV · MXF · AVI · WAV · MP3 · M4A · FLAC", font(11.0), FAINT);
}

fn spinner(p: &egui::Painter, c: Pos2, t: f64) {
    p.circle_stroke(c, 13.5, stroke(3.0, PANEL3));
    let a0 = (t * std::f64::consts::TAU / 0.9) as f32;
    let pts: Vec<Pos2> = (0..=12)
        .map(|i| {
            let a = a0 + i as f32 / 12.0 * std::f32::consts::FRAC_PI_2;
            Pos2::new(c.x + 13.5 * a.cos(), c.y + 13.5 * a.sin())
        })
        .collect();
    p.add(Shape::line(pts, stroke(3.0, ACCENT)));
}

fn player_area(app: &mut App, ui: &mut Ui, r: Rect) {
    let v = app.view.as_ref().unwrap();
    let p = ui.painter();
    p.rect_filled(r, CornerRadius::same(12), Color32::BLACK);
    if let Some(tex) = &app.video_tex {
        let [w, h] = tex.size();
        let scale = (r.width() / w as f32).min(r.height() / h as f32);
        let ir = Rect::from_center_size(r.center(), Vec2::new(w as f32 * scale, h as f32 * scale));
        p.image(tex.id(), ir, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
    }
    let overlay = |p: &egui::Painter| {
        p.rect_filled(r, CornerRadius::same(12), Color32::from_rgb(0x0b, 0x0d, 0x12));
        for i in 0..8 {
            let f = i as f32 / 8.0;
            p.rect_filled(Rect::from_center_size(r.center(), r.size() * (1.0 - f * 0.85)), CornerRadius::same(80), rgba(24, 28, 37, 0.14));
        }
    };
    let ready_media = v.playback.is_some();
    if v.stage == Stage::Error {
        overlay(p);
        let g = p.layout(format!("Erreur : {}", v.error), font(13.5), MUTED, r.width() - 40.0);
        p.galley(r.center() - g.size() / 2.0, g, MUTED);
    } else if !ready_media {
        overlay(p);
        spinner(p, r.center() - Vec2::new(0.0, 14.0), ui.input(|i| i.time));
        p.text(r.center() + Vec2::new(0.0, 22.0), Align2::CENTER_CENTER, v.stage.label(), font(13.5), MUTED);
        ui.ctx().request_repaint();
    } else if v.info.as_ref().map(|i| !i.has_video).unwrap_or(false) {
        overlay(p);
        let c = r.center() - Vec2::new(0.0, 14.0);
        p.circle_filled(c, 32.0, theme::accent_soft());
        p.text(c, Align2::CENTER_CENTER, "♪", font(26.0), ACCENT2);
        p.text(c + Vec2::new(0.0, 50.0), Align2::CENTER_CENTER, "Fichier audio", font(13.5), MUTED);
    }
    p.rect_stroke(r, CornerRadius::same(12), stroke(1.0, BORDER), StrokeKind::Inside);
    // clic sur l'image : lecture / pause
    if ui.interact(r, egui::Id::new("player-area"), Sense::click()).clicked() && ready_media {
        app.player.toggle();
    }
}

fn timecode(ui: &mut Ui, label: &str, value: &str, color: Color32) {
    let (r, _) = ui.allocate_exact_size(Vec2::new(92.0, 38.0), Sense::hover());
    let p = ui.painter();
    p.rect(r, CornerRadius::same(8), PANEL, stroke(1.0, BORDER), StrokeKind::Inside);
    p.text(Pos2::new(r.left() + 10.0, r.top() + 11.0), Align2::LEFT_CENTER, label, font(9.5), FAINT);
    p.text(Pos2::new(r.left() + 10.0, r.top() + 26.0), Align2::LEFT_CENTER, value, bfont(13.5), color);
}

fn transport(app: &mut App, ui: &mut Ui) {
    ui.horizontal_centered(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        if theme::icon_button(ui, Vec2::splat(34.0), "⏮", 13.0, false).on_hover_text("Coupe précédente (←)").clicked() {
            app.jump_cut(-1);
        }
        let icon = if app.player.playing() { "❚❚" } else { "▶" };
        if theme::icon_button(ui, Vec2::new(44.0, 38.0), icon, 14.0, true).on_hover_text("Lecture / pause (Espace)").clicked() {
            app.player.toggle();
        }
        if theme::icon_button(ui, Vec2::splat(34.0), "⏭", 13.0, false).on_hover_text("Coupe suivante (→)").clicked() {
            app.jump_cut(1);
        }
        ui.add_space(8.0);
        let t = app.player.time();
        timecode(ui, "O R I G I N A L", &fmt(t, true), TEXT);
        timecode(ui, "M O N T É", &fmt(app.edited_time(t), true), ACCENT2);
        ui.add_space(8.0);
        let mut pv = app.preview;
        if theme::switch(ui, &mut pv, "Aperçu monté", MUTED, font(12.5), 200.0) {
            app.preview = pv;
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let d = app.duration();
            let w = app.tl.width;
            if theme::icon_button(ui, Vec2::splat(28.0), "+", 13.0, false).on_hover_text("Zoomer").clicked() {
                app.tl.zoom(1.6, w / 2.0, d);
            }
            if theme::icon_button(ui, Vec2::splat(28.0), "⤢", 13.0, false).on_hover_text("Tout voir").clicked() {
                if let Some(id) = app.sel {
                    app.tl.fit(d, id);
                }
            }
            if theme::icon_button(ui, Vec2::splat(28.0), "−", 13.0, false).on_hover_text("Dézoomer").clicked() {
                app.tl.zoom(1.0 / 1.6, w / 2.0, d);
            }
            if ui.ctx().screen_rect().width() > 1360.0 {
                ui.add_space(8.0);
                legend(ui);
            }
        });
    });
}

fn legend(ui: &mut Ui) {
    // de droite à gauche
    for (label, kind) in [("Hésitation", 2), ("Silence", 1), ("Gardé", 0)] {
        let w = theme::text_width(ui, label, font(11.5)) + 16.0;
        let (r, _) = ui.allocate_exact_size(Vec2::new(w, 16.0), Sense::hover());
        let p = ui.painter();
        let sw = Rect::from_min_size(Pos2::new(r.left(), r.center().y - 5.0), Vec2::splat(10.0));
        let grey = Color32::from_rgb(0x55, 0x5b, 0x68);
        match kind {
            0 => {
                p.rect_filled(sw, CornerRadius::same(3), Color32::from_rgb(0xa6, 0xb2, 0xd4));
            }
            1 => {
                p.rect_stroke(sw, CornerRadius::same(3), stroke(1.0, grey), StrokeKind::Inside);
                p.line_segment([sw.left_bottom() + Vec2::new(2.0, -2.0), sw.right_top() + Vec2::new(-2.0, 2.0)], stroke(1.5, grey));
            }
            _ => {
                p.rect_filled(sw, CornerRadius::same(3), FILLER);
            }
        }
        p.text(Pos2::new(sw.right() + 5.0, r.center().y), Align2::LEFT_CENTER, label, font(11.5), MUTED);
        ui.add_space(4.0);
    }
}

fn timeline_area(app: &mut App, ui: &mut Ui, r: Rect) {
    let resp = ui.interact(r, egui::Id::new("timeline"), Sense::click_and_drag());
    let d = app.duration();
    app.tl.width = r.width();
    let (id, has_info, busy, label) = {
        let v = app.view.as_ref().unwrap();
        (v.id, v.info.is_some(), v.stage.busy(), v.stage.label())
    };
    if has_info {
        if app.tl.fitted != Some(id) {
            app.tl.fit(d, id);
        } else {
            app.tl.clamp(d);
        }
        let x = |pos: Pos2| (pos.x - r.left()).clamp(0.0, r.width());
        if resp.drag_started() || resp.clicked() || resp.dragged() {
            if let Some(pos) = resp.interact_pointer_pos() {
                let t = app.tl.x_to_t(x(pos));
                app.seek(t);
            }
        }
        app.tl.dragging = resp.dragged();
        if resp.double_clicked() {
            if let Some(pos) = resp.interact_pointer_pos() {
                let t = app.tl.x_to_t(x(pos));
                if let Some(i) = app.cuts.iter().position(|c| t >= c.start && t <= c.end) {
                    let on = !app.cuts[i].on;
                    app.set_cut(i, on);
                }
            }
        }
        if resp.hovered() {
            let (zoom, scroll, hover) = ui.input(|i| (i.zoom_delta(), i.smooth_scroll_delta, i.pointer.hover_pos()));
            if zoom != 1.0 {
                let ax = hover.map(x).unwrap_or(r.width() / 2.0);
                app.tl.zoom(if zoom > 1.0 { 1.25 } else { 0.8 }, ax, d);
            } else if scroll.x != 0.0 || scroll.y != 0.0 {
                app.tl.start -= (scroll.y + scroll.x) as f64 / app.tl.pps;
                app.tl.clamp(d);
            }
        }
        resp.on_hover_cursor(CursorIcon::PointingHand);
    }
    let p = ui.painter().with_clip_rect(r);
    if has_info {
        let v = app.view.as_ref().unwrap();
        let args = timeline::DrawArgs {
            duration: d,
            cuts: &app.cuts,
            db: &v.db,
            threshold: if app.auto { v.threshold } else { app.threshold_slider },
            playhead: app.player.time(),
        };
        timeline::draw(&p, r, &app.tl, &args);
    } else {
        p.rect(r, CornerRadius::same(12), PANEL, stroke(1.0, BORDER), StrokeKind::Inside);
    }
    if busy {
        let w = theme::text_width(ui, label, font(12.0)) + 38.0;
        let sr = Rect::from_min_size(Pos2::new(r.right() - 10.0 - w, r.top() + 28.0), Vec2::new(w, 24.0));
        p.rect(sr, CornerRadius::same(12), rgba(17, 20, 27, 0.85), stroke(1.0, rgba(124, 108, 255, 0.4)), StrokeKind::Inside);
        p.circle_filled(Pos2::new(sr.left() + 14.0, sr.center().y), 4.0, ACCENT.gamma_multiply(pulse(ui)));
        p.text(Pos2::new(sr.left() + 26.0, sr.center().y), Align2::LEFT_CENTER, label, font(12.0), ACCENT2);
    }
}

fn kbd(p: &egui::Painter, ui: &Ui, x: f32, y: f32, s: &str) -> f32 {
    let w = theme::text_width(ui, s, font(10.5)) + 10.0;
    let r = Rect::from_min_size(Pos2::new(x, y - 8.0), Vec2::new(w, 17.0));
    p.rect(r, CornerRadius::same(5), PANEL2, stroke(1.0, BORDER2), StrokeKind::Inside);
    p.line_segment([r.left_bottom() + Vec2::new(2.0, -0.5), r.right_bottom() + Vec2::new(-2.0, -0.5)], stroke(1.0, BORDER2));
    p.text(r.center(), Align2::CENTER_CENTER, s, font(10.5), MUTED);
    r.right() + 3.0
}

fn hints(ui: &Ui, r: Rect) {
    let p = ui.painter();
    let y = r.center().y;
    let mut x = r.left();
    let items: [(&[&str], &str); 5] = [
        (&["Clic"], "se placer"),
        (&["Double-clic"], "garder / recouper"),
        (&["Ctrl"], "+ molette : zoom"),
        (&["Espace"], "lecture"),
        (&["←", "→"], "coupe suivante"),
    ];
    for (keys, text) in items {
        for k in keys {
            x = kbd(p, ui, x, y, k);
        }
        let g = p.text(Pos2::new(x + 4.0, y), Align2::LEFT_CENTER, text, font(11.5), FAINT);
        x = g.right() + 16.0;
    }
}
