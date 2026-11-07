//! Panneau de droite (réglages, coupes, texte, export) et fenêtres par-dessus (à propos, mise à jour, dépôt, toast).

use super::jobs::{JobRef, Stage};
use super::panels::{dashed_rect, elide, hline, in_rect};
use super::theme::{self, bfont, font, rgba, stroke, Btn, ACCENT, ACCENT2, BORDER, BORDER2, FAINT, FILLER, MUTED, PANEL, PANEL2, PANEL3, TEXT};
use super::timeline::fmt;
use super::{App, CutFilter, Modal, Tab};
use crate::engine::cutter::{to_frames, Kind};
use crate::engine::export::{render_mp4, write_fcpxml, write_premiere_xml, write_vegas_edl};
use crate::engine::intervals::merge;
use crate::engine::text::{write_srt, write_txt};
use crate::engine::updates::{REPO_URL, VERSION};
use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, CursorIcon, Frame, Layout, Margin, Pos2, Rect, RichText, ScrollArea, Sense,
    StrokeKind, Ui, UiBuilder, Vec2,
};

pub fn side_panel(app: &mut App, ctx: &egui::Context) {
    let w = if ctx.screen_rect().width() > 1100.0 { 352.0 } else { 320.0 };
    let compact = ctx.screen_rect().height() < 820.0;
    egui::SidePanel::right("side")
        .exact_width(w)
        .resizable(false)
        .show_separator_line(false)
        .frame(Frame::new().fill(PANEL))
        .show(ctx, |ui| {
            let full = ui.max_rect();
            ui.painter().line_segment([Pos2::new(full.left() + 0.5, full.top()), Pos2::new(full.left() + 0.5, full.bottom())], stroke(1.0, BORDER));
            let tabs_r = Rect::from_min_size(full.min, Vec2::new(full.width(), 50.0));
            tabs(app, ui, tabs_r);
            let n_files = app.view.as_ref().map(|v| v.export.files.len()).unwrap_or(0);
            let status_h = (n_files.clamp(1, 5) as f32) * 20.0;
            let footer_h = if compact {
                14.0 + 52.0 + 10.0 + 36.0 + 6.0 + 34.0 + 6.0 + 34.0 + 10.0 + status_h + 14.0
            } else {
                14.0 + 52.0 + 28.0 + 24.0 + 92.0 + 32.0 + 34.0 + 6.0 + 34.0 + 10.0 + status_h + 14.0
            };
            let footer_r = Rect::from_min_max(Pos2::new(full.left(), full.bottom() - footer_h), full.max);
            let body_r = Rect::from_min_max(Pos2::new(full.left() + 1.0, tabs_r.bottom()), Pos2::new(full.right(), footer_r.top()));
            in_rect(ui, body_r, |ui| {
                ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    Frame::new().inner_margin(Margin::same(14)).show(ui, |ui| {
                        ui.set_width(body_r.width() - 28.0);
                        match app.tab {
                            Tab::Settings => settings_tab(app, ui),
                            Tab::Cuts => cuts_tab(app, ui),
                            Tab::Text => text_tab(app, ui),
                        }
                    });
                });
            });
            hline(ui, footer_r.top() + 0.5, footer_r.left(), footer_r.right());
            ui.painter().rect_filled(Rect::from_min_max(footer_r.min + Vec2::new(1.0, 1.0), footer_r.max), CornerRadius::ZERO, PANEL);
            in_rect(ui, footer_r.shrink(14.0), |ui| export_footer(app, ui, compact));
        });
}

fn tabs(app: &mut App, ui: &mut Ui, r: Rect) {
    let n_on = app.cuts.iter().filter(|c| c.on).count();
    let inner = Rect::from_min_max(Pos2::new(r.left() + 12.0, r.top() + 8.0), Pos2::new(r.right() - 12.0, r.bottom()));
    let tw = (inner.width() - 8.0) / 3.0;
    for (i, (tab, label)) in [(Tab::Settings, "Réglages"), (Tab::Cuts, "Coupes"), (Tab::Text, "Texte")].into_iter().enumerate() {
        let tr = Rect::from_min_size(Pos2::new(inner.left() + i as f32 * (tw + 4.0), inner.top()), Vec2::new(tw, inner.height()));
        let resp = ui.interact(tr, egui::Id::new(("tab", i)), Sense::click());
        let active = app.tab == tab;
        let col = if active || resp.hovered() { TEXT } else { MUTED };
        let p = ui.painter();
        let f = if active { bfont(13.5) } else { font(13.5) };
        if tab == Tab::Cuts {
            let n = n_on.to_string();
            let lw = theme::text_width(ui, label, f.clone());
            let bw = theme::text_width(ui, &n, font(10.5)) + 14.0;
            let x0 = tr.center().x - (lw + 6.0 + bw) / 2.0;
            p.text(Pos2::new(x0, tr.center().y), Align2::LEFT_CENTER, label, f, col);
            let br = Rect::from_min_size(Pos2::new(x0 + lw + 6.0, tr.center().y - 8.0), Vec2::new(bw, 16.0));
            p.rect_filled(br, CornerRadius::same(8), PANEL3);
            p.text(br.center(), Align2::CENTER_CENTER, n, font(10.5), col);
        } else {
            p.text(tr.center(), Align2::CENTER_CENTER, label, f, col);
        }
        if active {
            p.rect_filled(Rect::from_min_size(Pos2::new(tr.left(), tr.bottom() - 2.0), Vec2::new(tr.width(), 2.0)), CornerRadius::ZERO, ACCENT);
        }
        if resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
            app.tab = tab;
        }
    }
    hline(ui, r.bottom() - 0.5, r.left(), r.right());
}

fn card(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    Frame::new().fill(PANEL2).stroke(stroke(1.0, BORDER)).corner_radius(12).inner_margin(Margin::same(14)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        add(ui)
    });
    ui.add_space(12.0);
}

/// Ligne « titre … valeur » d'un réglage, avec la case « auto » du seuil.
fn field_head(ui: &mut Ui, label: &str, value: &str, auto: Option<&mut bool>) -> bool {
    let w = ui.available_width();
    let (r, _) = ui.allocate_exact_size(Vec2::new(w, 22.0), Sense::hover());
    ui.painter().text(Pos2::new(r.left(), r.center().y), Align2::LEFT_CENTER, label, font(12.5), MUTED);
    theme::pill(ui, Pos2::new(r.right(), r.center().y), value);
    let mut changed = false;
    if let Some(a) = auto {
        let vw = theme::text_width(ui, value, font(12.0)).max(46.0) + 12.0;
        let cw = 20.0 + theme::text_width(ui, "auto", font(11.5));
        let cr = Rect::from_min_size(Pos2::new(r.right() - vw - 8.0 - cw, r.center().y - 8.0), Vec2::new(cw, 16.0));
        ui.scope_builder(UiBuilder::new().max_rect(cr), |ui| {
            changed = theme::checkbox(ui, a, "auto");
        });
    }
    ui.add_space(2.0);
    changed
}

fn settings_tab(app: &mut App, ui: &mut Ui) {
    let mut changed = false;
    card(ui, |ui| {
        let w = ui.available_width();
        changed |= theme::switch(ui, &mut app.edit.cut_silences, "Couper les silences", TEXT, font(13.5), w);
        ui.add_space(14.0);
        let thr_label = if app.auto {
            app.view.as_ref().map(|v| format!("{} dB", v.threshold.round() as i64)).unwrap_or_else(|| "auto".into())
        } else {
            format!("{} dB", app.threshold_slider.round() as i64)
        };
        let mut auto = app.auto;
        if field_head(ui, "Seuil de silence", &thr_label, Some(&mut auto)) {
            app.auto = auto;
            if let (true, Some(v)) = (auto, &app.view) {
                app.threshold_slider = v.threshold.round();
            }
            changed = true;
        }
        changed |= theme::slider(ui, &mut app.threshold_slider, -70.0, -15.0, 1.0, !app.auto);
        ui.add_space(14.0);
        field_head(ui, "Silence minimum", &format!("{:.2} s", app.edit.min_silence), None);
        changed |= theme::slider(ui, &mut app.edit.min_silence, 0.2, 2.0, 0.05, true);
        ui.add_space(14.0);
        field_head(ui, "Marge autour de la parole", &format!("{} ms", (app.edit.margin * 1000.0).round() as i64), None);
        changed |= theme::slider(ui, &mut app.edit.margin, 0.0, 0.5, 0.01, true);
    });
    card(ui, |ui| {
        let w = ui.available_width();
        changed |= theme::switch(ui, &mut app.edit.cut_fillers, "Couper les hésitations (« euh », « hum »)", TEXT, font(13.5), w);
        ui.add_space(12.0);
        changed |= theme::switch(ui, &mut app.edit.cut_noises, "Couper les bruits de bouche", TEXT, font(13.5), w);
        ui.add_space(10.0);
        let device = if crate::engine::whisper::gpu_available() { "sur ta carte graphique" } else { "sur ton processeur" };
        ui.add(egui::Label::new(RichText::new(format!("Détectés par CrisperWhisper, en local {device}.")).size(11.5).color(FAINT)).wrap());
        if let Some(v) = &app.view {
            if v.stage == Stage::Transcription {
                ui.add_space(8.0);
                ui.add(egui::Label::new(RichText::new(&v.label).size(12.0).color(ACCENT2)).wrap());
                // pourcentage lu dans le libellé (« … 42 % »)
                let pct = v.label.split('%').next().and_then(|s| s.split_whitespace().last()).and_then(|n| n.parse::<f32>().ok());
                if let Some(p) = pct {
                    ui.add_space(4.0);
                    ui.add(egui::ProgressBar::new(p / 100.0).desired_height(5.0));
                }
            } else if v.stage == Stage::Ready && app.edit.cut_fillers && !v.has_words {
                ui.add_space(6.0);
                let msg = if v.words_error.is_empty() {
                    "Pas de transcription pour ce fichier : seuls les silences sont coupés.".to_string()
                } else {
                    format!("Transcription impossible ({}) : seuls les silences sont coupés.", v.words_error)
                };
                ui.add(egui::Label::new(RichText::new(msg).size(11.5).color(FILLER)).wrap());
            }
        }
    });
    if changed {
        app.on_settings();
    }
}

fn cuts_tab(app: &mut App, ui: &mut Ui) {
    if app.view.is_none() {
        return;
    }
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        for (label, on) in [("Tout couper", true), ("Tout garder", false)] {
            let w = theme::text_width(ui, label, font(12.0)) + 20.0;
            if theme::button(ui, Vec2::new(w, 28.0), Btn::Ghost, label, "", true, font(12.0)).clicked() {
                app.set_all_cuts(on);
            }
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let label = match app.filter {
                CutFilter::All => "Toutes",
                CutFilter::Filler => "Hésitations",
                CutFilter::Silence => "Silences",
            };
            egui::ComboBox::from_id_salt("cuts-filter").selected_text(RichText::new(label).size(12.0)).width(96.0).show_ui(ui, |ui| {
                ui.selectable_value(&mut app.filter, CutFilter::All, "Toutes");
                ui.selectable_value(&mut app.filter, CutFilter::Filler, "Hésitations");
                ui.selectable_value(&mut app.filter, CutFilter::Silence, "Silences");
            });
        });
    });
    ui.add_space(10.0);
    let w = ui.available_width();
    let (mut toggle, mut listen, mut goto) = (None, None, None);
    let hover = ui.input(|i| i.pointer.hover_pos());
    let over = |rr: Rect| hover.map(|h| rr.contains(h)).unwrap_or(false);
    for (i, c) in app.cuts.iter().enumerate() {
        match app.filter {
            CutFilter::Filler if c.kind != Kind::Filler => continue,
            CutFilter::Silence if c.kind != Kind::Silence => continue,
            _ => {}
        }
        let h = if c.text.is_empty() { 50.0 } else { 68.0 };
        let (r, resp) = ui.allocate_exact_size(Vec2::new(w, h), Sense::click());
        ui.add_space(6.0);
        if !ui.is_rect_visible(r) {
            continue;
        }
        let a = if c.on { 1.0 } else { 0.5 };
        let p = ui.painter();
        p.rect(r, CornerRadius::same(10), PANEL2, stroke(1.0, if resp.hovered() { BORDER2 } else { BORDER }), StrokeKind::Inside);
        let cb = Rect::from_min_size(Pos2::new(r.left() + 10.0, r.top() + 17.0), Vec2::splat(16.0));
        if c.on {
            p.rect_filled(cb, CornerRadius::same(3), ACCENT);
            p.line_segment([cb.left_center() + Vec2::new(3.5, 0.0), Pos2::new(cb.left() + 6.5, cb.bottom() - 4.0)], stroke(2.0, Color32::WHITE));
            p.line_segment([Pos2::new(cb.left() + 6.5, cb.bottom() - 4.0), Pos2::new(cb.right() - 3.5, cb.top() + 4.0)], stroke(2.0, Color32::WHITE));
        } else {
            p.rect(cb, CornerRadius::same(3), Color32::WHITE.gamma_multiply(0.5), stroke(1.0, MUTED), StrokeKind::Inside);
        }
        let x = cb.right() + 10.0;
        let (label, color) = if c.kind == Kind::Filler { ("Hésitation", FILLER) } else { ("Silence", Color32::from_rgb(0x7a, 0x81, 0x91)) };
        p.rect_filled(Rect::from_min_size(Pos2::new(x, r.top() + 13.0), Vec2::splat(10.0)), CornerRadius::same(3), color.gamma_multiply(a));
        let g = p.text(Pos2::new(x + 16.0, r.top() + 18.0), Align2::LEFT_CENTER, label, bfont(12.5), TEXT.gamma_multiply(a));
        if !c.on {
            p.line_segment([Pos2::new(g.left(), g.center().y + 1.0), Pos2::new(g.right(), g.center().y + 1.0)], stroke(1.0, TEXT.gamma_multiply(a)));
        }
        let when = format!("{} → {} · {:.2} s", fmt(c.start, true), fmt(c.end, true), c.end - c.start);
        p.text(Pos2::new(x, r.top() + 36.0), Align2::LEFT_CENTER, when, font(11.5), MUTED.gamma_multiply(a));
        if !c.text.is_empty() {
            p.text(Pos2::new(x, r.top() + 54.0), Align2::LEFT_CENTER, &c.text, font(12.0), FILLER.gamma_multiply(a));
        }
        let lr = Rect::from_min_size(Pos2::new(r.right() - 38.0, r.top() + 11.0), Vec2::splat(28.0));
        let lo = over(lr);
        p.rect(lr, CornerRadius::same(9), if lo { PANEL3 } else { PANEL2 }, stroke(1.0, if lo { BORDER2 } else { BORDER }), StrokeKind::Inside);
        p.text(lr.center(), Align2::CENTER_CENTER, "▶", font(10.0), TEXT);
        if resp.clicked() {
            if over(cb.expand(4.0)) {
                toggle = Some((i, !c.on));
            } else if lo {
                listen = Some(i);
            } else {
                goto = Some(c.start);
            }
        }
        let resp = resp.on_hover_cursor(CursorIcon::PointingHand);
        if lo {
            resp.on_hover_text("Écouter");
        } else if over(cb) {
            resp.on_hover_text("Couper");
        }
    }
    if let Some((i, on)) = toggle {
        app.set_cut(i, on);
    }
    if let Some(i) = listen {
        app.listen(i);
    }
    if let Some(t) = goto {
        app.seek((t - 0.5).max(0.0));
        let d = app.duration();
        app.tl.center_on(t, d);
    }
}

fn text_tab(app: &mut App, ui: &mut Ui) {
    let Some(v) = &app.view else { return };
    if v.words.is_empty() {
        let msg = if v.settings.cut_fillers {
            if v.stage == Stage::Transcription {
                "Transcription en cours…"
            } else {
                "Pas de transcription."
            }
        } else {
            "Active « Couper les hésitations » pour transcrire."
        };
        ui.label(RichText::new(msg).color(MUTED));
        return;
    }
    // mot en cours de lecture
    let t = app.player.time();
    let ws = &v.words;
    let idx = ws.partition_point(|w| w.start <= t);
    let now = if idx > 0 && t <= ws[idx - 1].end + 0.3 { Some(idx - 1) } else { None };
    let mut goto = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(4.0, 7.0);
        let mut prev_end = 0.0;
        for (i, w) in ws.iter().enumerate() {
            if w.start - prev_end > 1.0 {
                let s = format!("⏸ {:.1} s", w.start - prev_end);
                let gw = theme::text_width(ui, &s, font(10.5)) + 10.0;
                let (r, _) = ui.allocate_exact_size(Vec2::new(gw, 20.0), Sense::hover());
                ui.painter().rect_filled(r.shrink2(Vec2::new(0.0, 2.0)), CornerRadius::same(4), PANEL2);
                ui.painter().text(r.center(), Align2::CENTER_CENTER, s, font(10.5), FAINT);
            }
            let mid = (w.start + w.end) / 2.0;
            let cut = app.cuts.iter().find(|c| c.kind == Kind::Filler && mid >= c.start && mid <= c.end);
            let (color, struck) = match cut {
                Some(c) => (if c.on { FILLER.gamma_multiply(0.7) } else { FILLER }, c.on),
                None => (TEXT, false),
            };
            let ww = theme::text_width(ui, &w.word, font(14.0)) + 2.0;
            let (r, resp) = ui.allocate_exact_size(Vec2::new(ww, 20.0), Sense::click());
            let p = ui.painter();
            if now == Some(i) {
                p.rect_filled(r, CornerRadius::same(4), rgba(124, 108, 255, 0.35));
            } else if resp.hovered() {
                p.rect_filled(r, CornerRadius::same(4), PANEL3);
            }
            let g = p.text(r.center(), Align2::CENTER_CENTER, &w.word, font(14.0), color);
            if struck {
                p.line_segment([Pos2::new(g.left(), g.center().y + 1.0), Pos2::new(g.right(), g.center().y + 1.0)], stroke(1.0, color));
            }
            if resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                goto = Some(w.start);
            }
            prev_end = w.end;
        }
    });
    if let Some(t) = goto {
        app.seek(t);
        let d = app.duration();
        app.tl.center_on(t, d);
    }
}

fn stat(ui: &Ui, r: Rect, label: &str, value: &str, gain: bool) {
    let p = ui.painter();
    if gain {
        p.rect(r, CornerRadius::same(10), theme::accent_soft(), stroke(1.0, rgba(124, 108, 255, 0.3)), StrokeKind::Inside);
    } else {
        p.rect(r, CornerRadius::same(10), PANEL2, stroke(1.0, BORDER), StrokeKind::Inside);
    }
    p.text(Pos2::new(r.left() + 10.0, r.top() + 14.0), Align2::LEFT_CENTER, label, font(10.0), FAINT);
    p.text(Pos2::new(r.left() + 10.0, r.top() + 34.0), Align2::LEFT_CENTER, value, bfont(17.0), if gain { ACCENT2 } else { TEXT });
}

fn nle_card(ui: &mut Ui, size: Vec2, logo: u8, name: &str, ext: &str, enabled: bool, compact: bool) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(size, if enabled { Sense::click() } else { Sense::hover() });
    let a = if enabled { 1.0 } else { 0.4 };
    let hov = enabled && resp.hovered();
    let r = if enabled && resp.is_pointer_button_down_on() { r.translate(Vec2::new(0.0, 1.0)) } else { r };
    let p = ui.painter();
    let (fill, border) = if hov { (PANEL3, BORDER2) } else { (PANEL2, BORDER) };
    p.rect(r, CornerRadius::same(10), fill.gamma_multiply(a), stroke(1.0, border.gamma_multiply(a)), StrokeKind::Inside);
    let flat = name.replace('\n', " ");
    let ls = if compact { 24.0 } else { 30.0 };
    let lr = if compact {
        let nw = theme::text_width(ui, &flat, bfont(11.5));
        Rect::from_min_size(Pos2::new(r.center().x - (ls + 6.0 + nw) / 2.0, r.center().y - ls / 2.0), Vec2::splat(ls))
    } else {
        Rect::from_min_size(Pos2::new(r.center().x - ls / 2.0, r.top() + 10.0), Vec2::splat(ls))
    };
    let fs = if compact { 11.0 } else { 12.5 };
    match logo {
        0 => {
            theme::gradient_rect(
                p,
                lr,
                8.0,
                &[
                    (0.0, Color32::from_rgb(0xff, 0x6b, 0x3d).gamma_multiply(a)),
                    (0.5, Color32::from_rgb(0xff, 0xb1, 0x3d).gamma_multiply(a)),
                    (1.0, Color32::from_rgb(0x3d, 0xc3, 0xff).gamma_multiply(a)),
                ],
            );
            p.text(lr.center(), Align2::CENTER_CENTER, "DR", bfont(fs), Color32::from_rgb(0x12, 0x12, 0x12));
        }
        1 => {
            let lilac = Color32::from_rgb(0x99, 0x99, 0xff).gamma_multiply(a);
            p.rect(lr, CornerRadius::same(8), Color32::from_rgb(0, 0, 0x5b).gamma_multiply(a), stroke(1.5, lilac), StrokeKind::Inside);
            p.text(lr.center(), Align2::CENTER_CENTER, "Pr", bfont(fs), lilac);
        }
        _ => {
            theme::gradient_rect(p, lr, 8.0, &[(0.0, Color32::from_rgb(0x1d, 0x1d, 0x1f)), (1.0, Color32::from_rgb(0x3a, 0x3a, 0x3e))]);
            p.rect_stroke(lr, CornerRadius::same(8), stroke(1.5, Color32::from_rgb(0xd6, 0xd6, 0xd6).gamma_multiply(a)), StrokeKind::Inside);
            p.text(lr.center(), Align2::CENTER_CENTER, "V", bfont(fs + 1.5), Color32::WHITE.gamma_multiply(a));
        }
    }
    if compact {
        p.text(Pos2::new(lr.right() + 6.0, r.center().y), Align2::LEFT_CENTER, flat, bfont(11.5), TEXT.gamma_multiply(a));
    } else {
        for (li, line) in name.split('\n').enumerate() {
            p.text(Pos2::new(r.center().x, lr.bottom() + 13.0 + li as f32 * 14.0), Align2::CENTER_CENTER, line, bfont(11.5), TEXT.gamma_multiply(a));
        }
        p.text(Pos2::new(r.center().x, r.bottom() - 12.0), Align2::CENTER_CENTER, ext, font(10.5), FAINT.gamma_multiply(a));
    }
    if enabled {
        resp.on_hover_cursor(CursorIcon::PointingHand)
    } else {
        resp
    }
}

fn export_footer(app: &mut App, ui: &mut Ui, compact: bool) {
    let w = ui.available_width();
    let orig = app.view.as_ref().and_then(|v| v.info.as_ref()).map(|i| i.duration);
    let fin: f64 = if orig.is_some() { app.final_keep().iter().map(|(s, e)| e - s).sum() } else { 0.0 };
    let (r, _) = ui.allocate_exact_size(Vec2::new(w, 52.0), Sense::hover());
    let sw = (w - 12.0) / 3.0;
    let cell = |i: usize| Rect::from_min_size(Pos2::new(r.left() + i as f32 * (sw + 6.0), r.top()), Vec2::new(sw, 52.0));
    let dash = || "–".to_string();
    stat(ui, cell(0), "O R I G I N A L", &orig.map(|o| fmt(o, false)).unwrap_or_else(dash), false);
    stat(ui, cell(1), "A P R È S   C O U P E", &orig.map(|_| fmt(fin, false)).unwrap_or_else(dash), false);
    let gain = orig.filter(|o| *o > 0.0).map(|o| format!("−{} %", ((100.0 * (o - fin)) / o).round() as i64)).unwrap_or_else(dash);
    stat(ui, cell(2), "G A G N É", &gain, true);
    if !compact {
        ui.add_space(10.0);
        let (mr, _) = ui.allocate_exact_size(Vec2::new(w, 4.0), Sense::hover());
        ui.painter().rect_filled(mr, CornerRadius::same(2), PANEL3);
        if let Some(o) = orig.filter(|o| *o > 0.0) {
            let fw = mr.width() * (fin / o) as f32;
            if fw > 1.0 {
                theme::accent_rect(ui.painter(), Rect::from_min_size(mr.min, Vec2::new(fw, 4.0)), 2.0);
            }
        }
        ui.add_space(12.0);
        ui.label(RichText::new("TIMELINE POUR TON LOGICIEL DE MONTAGE").size(10.5).color(FAINT));
    } else {
        ui.add_space(4.0);
    }
    let (ready, rendering, has_words) =
        app.view.as_ref().map(|v| (v.stage == Stage::Ready, v.export.state == "rendu", !v.words.is_empty())).unwrap_or((false, false, false));
    let ok = ready && !rendering;
    let mut clicked: Option<&str> = None;
    let ch = if compact { 36.0 } else { 92.0 };
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let cw = (w - 12.0) / 3.0;
        let cards = [
            (0, "DaVinci\nFinal Cut", ".fcpxml", "fcpxml", "DaVinci Resolve : Fichier > Importer > Timeline · Final Cut Pro : Fichier > Importer > XML"),
            (1, "Premiere\nPro", ".xml", "premiere", "Premiere Pro : Fichier > Importer"),
            (2, "Vegas\nPro", ".txt", "vegas", "Vegas Pro : Fichier > Importer > Fichier texte EDL"),
        ];
        for (logo, name, ext, fmt_id, tip) in cards {
            if nle_card(ui, Vec2::new(cw, ch), logo, name, ext, ok, compact).on_hover_text(tip).clicked() {
                clicked = Some(fmt_id);
            }
        }
    });
    if !compact {
        ui.add_space(8.0);
        ui.label(RichText::new("FICHIERS").size(10.5).color(FAINT));
    }
    if theme::button(ui, Vec2::new(w, 34.0), Btn::Primary, "Vidéo montée", ".mp4", ok, font(13.5)).clicked() {
        clicked = Some("mp4");
    }
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let bw = (w - 6.0) / 2.0;
        let tip = "Active « Couper les hésitations » pour transcrire";
        for (label, ext, id) in [("Sous-titres", ".srt", "srt"), ("Texte", ".txt", "txt")] {
            let resp = theme::button(ui, Vec2::new(bw, 34.0), Btn::Normal, label, ext, ok && has_words, font(13.5));
            if resp.clicked() {
                clicked = Some(id);
            } else if !has_words && ready {
                resp.on_hover_text(tip);
            }
        }
    });
    ui.add_space(4.0);
    if let Some(f) = clicked {
        app.export(f);
    }
    let Some(v) = &app.view else { return };
    let ex = &v.export;
    if ex.state == "rendu" {
        ui.label(RichText::new("Rendu de la vidéo montée…").size(12.0).color(MUTED));
    } else if ex.state == "erreur" {
        ui.label(RichText::new(format!("Erreur : {}", ex.error)).size(12.0).color(theme::DANGER));
    } else if !ex.files.is_empty() {
        let mut reveal = None;
        for f in ex.files.iter().rev().take(5) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                ui.label(RichText::new("✓").size(12.0).color(theme::OK));
                let name = std::path::Path::new(f).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let avail = ui.available_width() - 64.0;
                ui.label(RichText::new(elide(ui, &name, font(12.0), avail)).size(12.0).color(MUTED)).on_hover_text(f);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let l = ui.add(egui::Label::new(RichText::new("Afficher").size(12.0).color(ACCENT2)).sense(Sense::click()));
                    if l.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                        reveal = Some(f.clone());
                    }
                });
            });
        }
        if let Some(f) = reveal {
            super::reveal(&f);
        }
    } else if ready {
        ui.label(RichText::new("Les fichiers sont créés à côté du rush.").size(12.0).color(MUTED));
    }
}

/// Écrit les exports demandés à côté du rush (le MP4 se rend en tâche de fond).
pub fn run_export(job: &JobRef, formats: &[&str], keep: &[(f64, f64)]) -> Result<(), String> {
    let mut j = job.lock().unwrap();
    let info = j.info.clone().ok_or("Analyse pas terminée")?;
    let keep = merge(keep);
    let frames = to_frames(&keep, info.fps, info.total_frames());
    if frames.is_empty() {
        return Err("Rien à exporter".into());
    }
    let outdir = info.path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let st = info.path.file_stem().unwrap_or_default().to_string_lossy().into_owned();
    let stem = |ext: &str| outdir.join(format!("{st}_crisptrim.{ext}"));
    let name = format!("{st} (CrispTrim)");
    let has = |f: &str| formats.contains(&f);
    let io = |r: std::io::Result<()>| r.map_err(|e| format!("Écriture impossible : {e}"));
    let mut files: Vec<String> = Vec::new();
    if has("txt") || has("srt") {
        let words = j.words.clone().filter(|w| !w.is_empty()).ok_or("Pas de transcription : active « Couper les hésitations » pour transcrire.")?;
        let aligned: Vec<(f64, f64)> = frames.iter().map(|&(a, b)| (info.fps.secs(a), info.fps.secs(b))).collect();
        if has("txt") {
            io(write_txt(&words, &aligned, &stem("txt")))?;
            files.push(stem("txt").display().to_string());
        }
        if has("srt") {
            io(write_srt(&words, &aligned, &stem("srt")))?;
            files.push(stem("srt").display().to_string());
        }
    }
    if has("fcpxml") {
        io(write_fcpxml(&info, &frames, &stem("fcpxml"), &name))?;
        files.push(stem("fcpxml").display().to_string());
    }
    if has("premiere") {
        io(write_premiere_xml(&info, &frames, &stem("xml"), &name))?;
        files.push(stem("xml").display().to_string());
    }
    if has("vegas") {
        io(write_vegas_edl(&info, &frames, &stem("vegas.txt")))?;
        files.push(stem("vegas.txt").display().to_string());
    }
    let mut all: Vec<String> = j.export.files.iter().filter(|f| !files.contains(f)).cloned().collect();
    all.extend(files);
    j.export.files = all;
    j.export.state = "fait".into();
    j.export.error.clear();
    if has("mp4") {
        j.export.state = "rendu".into();
        let mp4 = stem("mp4");
        let job2 = job.clone();
        std::thread::spawn(move || {
            let res = render_mp4(&info, &frames, &mp4);
            let mut j = job2.lock().unwrap();
            match res {
                Ok(()) => {
                    let m = mp4.display().to_string();
                    j.export.files.retain(|f| *f != m);
                    j.export.files.push(m);
                    j.export.state = "fait".into();
                }
                Err(e) => {
                    j.export.state = "erreur".into();
                    j.export.error = e;
                }
            }
            j.version += 1;
        });
    }
    j.version += 1;
    Ok(())
}

// =========================================================================== fenêtres, toast, dépôt

fn shadow() -> egui::epaint::Shadow {
    egui::epaint::Shadow { offset: [0, 10], blur: 40, spread: 0, color: rgba(0, 0, 0, 0.45) }
}

/// Fenêtre centrée sur fond assombri ; renvoie vrai si on la ferme (✕ ou clic à côté).
fn modal_frame(ctx: &egui::Context, id: &str, add: impl FnOnce(&mut Ui)) -> bool {
    let screen = ctx.screen_rect();
    let mut close = false;
    egui::Area::new(egui::Id::new((id, "backdrop"))).order(egui::Order::Foreground).fixed_pos(Pos2::ZERO).show(ctx, |ui| {
        let (r, resp) = ui.allocate_exact_size(screen.size(), Sense::click());
        ui.painter().rect_filled(r, CornerRadius::ZERO, rgba(6, 8, 12, 0.7));
        close |= resp.clicked();
    });
    let w = 500.0f32.min(screen.width() - 48.0);
    egui::Area::new(egui::Id::new((id, "modal"))).order(egui::Order::Tooltip).anchor(Align2::CENTER_CENTER, Vec2::ZERO).show(ctx, |ui| {
        Frame::new().fill(PANEL).stroke(stroke(1.0, BORDER2)).corner_radius(16).inner_margin(Margin::same(24)).shadow(shadow()).show(ui, |ui| {
            ui.set_width(w - 48.0);
            let top = ui.max_rect().right_top();
            let xr = Rect::from_min_size(Pos2::new(top.x - 16.0, top.y - 12.0), Vec2::splat(28.0));
            let resp = ui.interact(xr, egui::Id::new((id, "x")), Sense::click());
            if resp.hovered() {
                ui.painter().rect_filled(xr, CornerRadius::same(8), PANEL3);
            }
            ui.painter().text(xr.center(), Align2::CENTER_CENTER, "✕", font(13.0), if resp.hovered() { TEXT } else { MUTED });
            close |= resp.on_hover_text("Fermer").clicked();
            add(ui);
        });
    });
    close
}

fn link(ui: &mut Ui, text: RichText) -> bool {
    ui.add(egui::Label::new(text.color(ACCENT2)).sense(Sense::click())).on_hover_cursor(CursorIcon::PointingHand).clicked()
}

fn modal_head(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 14.0;
        let (r, _) = ui.allocate_exact_size(Vec2::splat(48.0), Sense::hover());
        theme::logo(ui.painter(), r, 14.0);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.add_space(3.0);
            add(ui)
        });
    });
    ui.add_space(14.0);
}

fn about(app: &mut App, ctx: &egui::Context) {
    let mut open: Option<String> = None;
    let mut check = false;
    let update = app.update.clone();
    let close = modal_frame(ctx, "about", |ui| {
        modal_head(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.label(RichText::new("Crisp").size(19.0).family(theme::bold()));
                ui.label(RichText::new("Trim").size(19.0).color(ACCENT2).family(theme::bold()));
            });
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.label(RichText::new(format!("Version {VERSION} ·")).color(MUTED));
                match &update {
                    None => {
                        ui.label(RichText::new("vérification…").color(MUTED));
                    }
                    Some(u) if u.available => {
                        let t = RichText::new(format!("{} disponible", u.latest.clone().unwrap_or_default())).strong();
                        if link(ui, t) {
                            open = Some("#update".into());
                        }
                    }
                    Some(u) if !u.error.is_empty() && u.latest.is_none() => {
                        ui.label(RichText::new("impossible de vérifier (hors ligne ?)").color(MUTED));
                    }
                    Some(_) => {
                        ui.label(RichText::new("à jour").color(theme::OK));
                    }
                }
            });
        });
        ui.label("Coupe automatiquement les silences et les « euh » de tes rushes, en local.");
        ui.add_space(14.0);
        Frame::new().fill(PANEL2).stroke(stroke(1.0, BORDER)).corner_radius(12).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let rows = [
                ("Créé par", "xLaugh", "https://github.com/xLaugh"),
                ("Code source", "github.com/xLaugh/CrispTrim", REPO_URL),
                ("Licence", "GNU GPL v3", "https://github.com/xLaugh/CrispTrim/blob/main/LICENSE"),
            ];
            for (i, (k, v, url)) in rows.into_iter().enumerate() {
                if i > 0 {
                    let y = ui.cursor().top();
                    hline(ui, y, ui.max_rect().left(), ui.max_rect().right());
                }
                Frame::new().inner_margin(Margin::symmetric(12, 9)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(k).color(MUTED));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let t = if i == 0 { RichText::new(v).strong() } else { RichText::new(v) };
                            if link(ui, t) {
                                open = Some(url.to_string());
                            }
                        });
                    });
                });
            }
        });
        ui.add_space(18.0);
        ui.label(RichText::new("C R É D I T S").size(11.0).color(FAINT));
        ui.add_space(4.0);
        let credits: [(&str, &str, &str); 4] = [
            ("CrisperWhisper 2.0", "https://huggingface.co/nyrahealth", " par Nyra Health : transcription mot à mot. Code sous MIT, poids du modèle sous licence non commerciale."),
            ("Whisper", "https://github.com/openai/whisper", " par OpenAI (MIT)"),
            ("FFmpeg", "https://ffmpeg.org", " : décodage et rendu vidéo"),
            ("egui", "https://github.com/emilk/egui", " et cpal : fenêtre et son, en Rust"),
        ];
        for (name, url, rest) in credits {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.label(RichText::new("•  ").color(MUTED).size(12.5));
                if link(ui, RichText::new(name).size(12.5)) {
                    open = Some(url.to_string());
                }
                ui.label(RichText::new(rest).color(MUTED).size(12.5));
            });
        }
        ui.add_space(20.0);
        ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let w = theme::text_width(ui, "Ouvrir GitHub", bfont(13.5)) + 28.0;
            if theme::button(ui, Vec2::new(w, 34.0), Btn::Primary, "Ouvrir GitHub", "", true, font(13.5)).clicked() {
                open = Some(REPO_URL.to_string());
            }
            let w = theme::text_width(ui, "Rechercher une mise à jour", font(13.5)) + 28.0;
            if theme::button(ui, Vec2::new(w, 34.0), Btn::Ghost, "Rechercher une mise à jour", "", true, font(13.5)).clicked() {
                check = true;
            }
        });
    });
    if close {
        app.modal = Modal::None;
    }
    if check {
        app.check_update(true);
        app.check_toast = true;
    }
    match open.as_deref() {
        Some("#update") => app.modal = Modal::Update,
        Some(u) => super::open_url(u),
        None => {}
    }
}

fn update_modal(app: &mut App, ctx: &egui::Context) {
    let Some(u) = app.update.clone() else {
        app.modal = Modal::None;
        return;
    };
    let (mut later, mut open, mut install) = (false, false, false);
    let close = modal_frame(ctx, "update", |ui| {
        modal_head(ui, |ui| {
            ui.label(RichText::new("Nouvelle version disponible").size(19.0).family(theme::bold()));
            let name = u.release.as_ref().map(|r| r.name.clone()).unwrap_or_default();
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.label(RichText::new(name).color(ACCENT2).strong());
                ui.label(RichText::new(format!("· tu as la v{}", u.current)).color(MUTED));
            });
        });
        let notes = u.release.as_ref().map(|r| r.notes.clone()).unwrap_or_default();
        if !notes.is_empty() {
            Frame::new().fill(PANEL2).stroke(stroke(1.0, BORDER)).corner_radius(10).inner_margin(Margin::same(12)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                    ui.add(egui::Label::new(RichText::new(notes).size(12.5)).wrap());
                });
            });
        }
        ui.add_space(12.0);
        let dl = app.update_dl.as_ref().map(|d| d.lock().unwrap().clone());
        let how = match &dl {
            Some((_, _, Some(e), _)) => format!("La mise à jour a échoué : {e}"),
            Some((a, b, None, _)) if *b > 0 => format!("Téléchargement… {} %  ({} / {} Mo)", a * 100 / b, a / 1_000_000, b / 1_000_000),
            Some(_) => "Téléchargement…".to_string(),
            None if crate::engine::updates::is_installed() => {
                "CrispTrim télécharge la nouvelle version, se ferme et se relance tout seul. Les réglages, les modèles et le cache sont gardés.".to_string()
            }
            None => "La nouvelle version remplace ce fichier CrispTrim.exe, puis se relance. Les réglages, les modèles et le cache sont gardés.".to_string(),
        };
        let failed = matches!(&dl, Some((_, _, Some(_), _)));
        ui.add(egui::Label::new(RichText::new(how).size(12.0).color(if failed { FILLER } else { MUTED })).wrap());
        if let Some((a, b, None, _)) = &dl {
            ui.add_space(6.0);
            ui.add(egui::ProgressBar::new(if *b > 0 { *a as f32 / *b as f32 } else { 0.0 }).desired_height(5.0));
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        ui.add_space(20.0);
        let busy = matches!(&dl, Some((_, _, None, _)));
        ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let w = theme::text_width(ui, "Mettre à jour", bfont(13.5)) + 28.0;
            install = theme::button(ui, Vec2::new(w, 34.0), Btn::Primary, "Mettre à jour", "", !busy, font(13.5)).clicked();
            let w = theme::text_width(ui, "Voir sur GitHub", font(13.5)) + 28.0;
            open = theme::button(ui, Vec2::new(w, 34.0), Btn::Ghost, "Voir sur GitHub", "", true, font(13.5)).clicked();
            let w = theme::text_width(ui, "Plus tard", font(13.5)) + 28.0;
            later = theme::button(ui, Vec2::new(w, 34.0), Btn::Ghost, "Plus tard", "", !busy, font(13.5)).clicked();
        });
    });
    // téléchargement terminé : l'installateur ou la nouvelle version prend le relais
    if app.update_dl.as_ref().is_some_and(|d| d.lock().unwrap().3) {
        std::process::exit(0);
    }
    if install {
        if let Some(tag) = u.latest.clone() {
            let state = std::sync::Arc::new(std::sync::Mutex::new((0u64, 0u64, None::<String>, false)));
            app.update_dl = Some(state.clone());
            let repaint = ctx.clone();
            std::thread::spawn(move || {
                let r = crate::engine::updates::install_update(&tag, &mut |a, b| {
                    let mut s = state.lock().unwrap();
                    s.0 = a;
                    s.1 = b;
                });
                let mut s = state.lock().unwrap();
                match r {
                    Ok(()) => s.3 = true,
                    Err(e) => s.2 = Some(e),
                }
                repaint.request_repaint();
            });
        }
    }
    if open {
        let url = u.release.as_ref().map(|r| r.url.clone()).unwrap_or_else(|| format!("{REPO_URL}/releases"));
        super::open_url(&url);
    }
    if later {
        app.skip_version = u.latest.clone();
        app.save_ui_state();
        app.modal = Modal::None;
    } else if close {
        app.modal = Modal::None;
    }
}

pub fn overlays(app: &mut App, ctx: &egui::Context) {
    // glisser-déposer
    if ctx.input(|i| !i.raw.hovered_files.is_empty()) {
        egui::Area::new(egui::Id::new("drop")).order(egui::Order::Foreground).fixed_pos(Pos2::ZERO).interactable(false).show(ctx, |ui| {
            let s = ctx.screen_rect();
            let p = ui.painter();
            p.rect_filled(s, CornerRadius::ZERO, rgba(8, 10, 14, 0.78));
            let t = s.shrink(30.0);
            p.rect_filled(t, CornerRadius::same(24), rgba(124, 108, 255, 0.07));
            dashed_rect(p, t, ACCENT, 2.0);
            p.text(t.center() - Vec2::new(0.0, 18.0), Align2::CENTER_CENTER, "⇣", font(30.0), ACCENT2);
            p.text(t.center() + Vec2::new(0.0, 18.0), Align2::CENTER_CENTER, "Dépose tes rushes pour les analyser", bfont(18.0), TEXT);
        });
    }
    match app.modal {
        Modal::About => about(app, ctx),
        Modal::Update => update_modal(app, ctx),
        Modal::None => {}
    }
    // toast
    if let Some((msg, err, at)) = app.toast.clone() {
        let life = if err { 6.0 } else { 3.0 };
        if at.elapsed().as_secs_f32() > life {
            app.toast = None;
        } else {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
            egui::Area::new(egui::Id::new("toast"))
                .order(egui::Order::Tooltip)
                .anchor(Align2::CENTER_BOTTOM, Vec2::new(0.0, -20.0))
                .interactable(false)
                .show(ctx, |ui| {
                    Frame::new()
                        .fill(PANEL3)
                        .stroke(stroke(1.0, if err { theme::DANGER } else { BORDER2 }))
                        .corner_radius(10)
                        .inner_margin(Margin::symmetric(16, 10))
                        .shadow(shadow())
                        .show(ui, |ui| {
                            ui.set_max_width(ctx.screen_rect().width() * 0.8);
                            ui.label(RichText::new(msg).color(TEXT));
                        });
                });
        }
    }
}
