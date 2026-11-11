//! Editor: an observatory for a memory that drifts.
//!
//! Centre: an orrery. The outer ring is the whole 2^21-sample memory, its
//! envelope coloured by the segments of the active program; inside, every
//! delay line is an orbit whose planet is the write head. Prime lengths keep
//! the planets from ever lining up again. Around it, the twenty-one programs
//! in five constellations.
//!
//! Left (MATTER): SCATTER, DRIFT, FLARE, IN, DILATION.
//! Right (GRAVITY): TIDE, ORBIT, MIX, OUT, PARALLAX.
//! Below: seven performance pads, then the tone, period and horizon strip.
//! Every gesture shows in the sky: an eclipse covers the sun, a redshift
//! reddens the orbits, dust sparkles, Roche draws tidal threads between the
//! slice that plays and the slice it was torn from.

use crate::engine::layouts::{Family, HW_RATE, Layout, MAIN, MEM, NAMES, PULSAR, QUAD};
use crate::engine::pulsar::{MAX_LOOP, RB};
use crate::scope::{BINS, Slot};
use crate::{SideralParams, SideralParamsParamId as P};
use egui::{Align2, Color32, CornerRadius, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2, pos2, vec2};
use std::f32::consts::{PI, TAU};
use std::sync::Arc;
use truce::prelude::*;
use truce_egui::EguiEditor;

const W: f32 = 1180.0;
const H: f32 = 790.0;

// --- Palette ------------------------------------------------------------------
const BG: Color32 = Color32::from_rgb(6, 7, 12);
const PANEL: Color32 = Color32::from_rgb(13, 15, 23);
const RULE: Color32 = Color32::from_rgb(34, 38, 54);
const TEXT: Color32 = Color32::from_rgb(232, 229, 220);
const DIM: Color32 = Color32::from_rgb(125, 130, 150);
const FAINT: Color32 = Color32::from_rgb(66, 70, 90);
const CAP: Color32 = Color32::from_rgb(236, 233, 226);
const CAP_SHADE: Color32 = Color32::from_rgb(170, 168, 160);
const CYAN: Color32 = Color32::from_rgb(143, 211, 255);
const AMBER: Color32 = Color32::from_rgb(255, 179, 71);
const RED: Color32 = Color32::from_rgb(255, 82, 70);
const ROSE: Color32 = Color32::from_rgb(255, 120, 150);
const GREEN: Color32 = Color32::from_rgb(107, 220, 138);
const VIOLET: Color32 = Color32::from_rgb(178, 156, 255);
const SAND: Color32 = Color32::from_rgb(236, 214, 150);
const PAD: Color32 = Color32::from_rgb(18, 21, 32);
const LED_OFF: Color32 = Color32::from_rgb(36, 40, 54);

const ORBIT: [Color32; 9] = [
    Color32::from_rgb(143, 211, 255),
    Color32::from_rgb(255, 179, 71),
    Color32::from_rgb(178, 156, 255),
    Color32::from_rgb(126, 224, 181),
    Color32::from_rgb(255, 143, 163),
    Color32::from_rgb(245, 227, 122),
    Color32::from_rgb(154, 184, 255),
    Color32::from_rgb(255, 160, 224),
    Color32::from_rgb(150, 150, 160),
];

const FAMILIES: [(&str, usize, usize); 5] =
    [("BINARY", 0, 3), ("TRAPEZIUM", 3, 3), ("NEBULA", 6, 3), ("STARDUST", 9, 3), ("PULSAR", 12, 9)];

pub fn create_editor(params: Arc<SideralParams>) -> Box<dyn Editor> {
    Box::new(EguiEditor::with_ui(params, (W as u32, H as u32), SideralUi::default()).with_visuals(visuals()))
}

fn visuals() -> egui::Visuals {
    let mut v = egui::Visuals::dark();
    v.panel_fill = BG;
    v.window_fill = PANEL;
    v.override_text_color = Some(TEXT);
    v
}

struct SideralUi {
    last_rec: usize,
    crunch_start: Option<f64>,
}

impl Default for SideralUi {
    fn default() -> Self {
        Self { last_rec: 3, crunch_start: None }
    }
}

type Ctx = PluginContext<SideralParams>;

impl truce_egui::EditorUi<SideralParams> for SideralUi {
    fn ui(&mut self, ui: &mut egui::Ui, st: &Ctx) {
        ui.ctx().request_repaint();
        let time = ui.input(|i| i.time) as f32;
        let p = ui.painter().clone();
        p.rect_filled(Rect::from_min_size(Pos2::ZERO, vec2(W, H)), 0.0, BG);
        starfield(&p, time);

        header(ui, st);
        matter(ui, st, Rect::from_min_size(pos2(14.0, 66.0), vec2(252.0, 456.0)));
        gravity(ui, st, Rect::from_min_size(pos2(W - 266.0, 66.0), vec2(252.0, 456.0)));
        orrery(ui, st, pos2(W * 0.5, 292.0), time);
        status_line(ui, st, Rect::from_min_size(pos2(290.0, 526.0), vec2(W - 580.0, 34.0)));
        pads(ui, st, self, Rect::from_min_size(pos2(14.0, 570.0), vec2(W - 28.0, 100.0)), time);
        strip(ui, st, Rect::from_min_size(pos2(14.0, 682.0), vec2(W - 28.0, 94.0)));
    }
}

// ===========================================================================
// Helpers
// ===========================================================================

fn stroke(w: f32, c: Color32) -> Stroke {
    Stroke::new(w, c)
}

fn alpha(c: Color32, a: f32) -> Color32 {
    let a = a.clamp(0.0, 1.0);
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), (a * 255.0) as u8)
}

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t) as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

fn polar(c: Pos2, r: f32, a: f32) -> Pos2 {
    c + vec2(a.cos(), a.sin()) * r
}

fn arc(c: Pos2, r: f32, start: f32, sweep: f32, n: usize) -> Vec<Pos2> {
    (0..=n).map(|i| polar(c, r, start + sweep * i as f32 / n as f32)).collect()
}

fn hash(i: u32) -> f32 {
    let mut x = i.wrapping_mul(0x9E37_79B9) ^ 0x85EB_CA6B;
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^= x >> 12;
    (x & 0xffff) as f32 / 65535.0
}

fn starfield(p: &egui::Painter, t: f32) {
    for i in 0..190u32 {
        let x = hash(i * 3) * W;
        let y = hash(i * 3 + 1) * H;
        let tw = 0.5 + 0.5 * (t * (0.3 + hash(i * 7) * 1.2) + hash(i * 5) * TAU).sin();
        let a = 0.06 + 0.22 * hash(i * 3 + 2) * tw;
        p.circle_filled(pos2(x, y), 0.6 + hash(i * 11) * 0.8, alpha(Color32::WHITE, a));
    }
}

fn section(p: &egui::Painter, r: Rect, title: &str) {
    p.rect_filled(r, CornerRadius::same(10), alpha(PANEL, 0.82));
    p.rect_stroke(r, CornerRadius::same(10), stroke(1.0, RULE), StrokeKind::Inside);
    if !title.is_empty() {
        p.text(pos2(r.left() + 14.0, r.top() + 15.0), Align2::LEFT_CENTER, title, FontId::proportional(10.0), DIM);
    }
}

fn set(st: &Ctx, id: P, v: f64) {
    let id: u32 = id.into();
    st.begin_edit(id);
    st.set_param(id, v);
    st.end_edit(id);
}

fn enum_count(st: &Ctx, id: u32) -> usize {
    st.params()
        .param_infos()
        .iter()
        .find(|i| i.id == id)
        .and_then(|i| i.range.step_count())
        .map_or(2, |n| n.get() as usize + 1)
}

fn enum_index(st: &Ctx, id: P) -> usize {
    let id: u32 = id.into();
    let n = enum_count(st, id);
    (st.get_param(id) * (n - 1) as f32).round() as usize
}

fn set_enum(st: &Ctx, id: P, index: usize) {
    let raw: u32 = id.into();
    let n = enum_count(st, raw);
    let norm = if n > 1 { index.min(n - 1) as f64 / (n - 1) as f64 } else { 0.0 };
    set(st, id, norm);
}

fn is_on(st: &Ctx, id: P) -> bool {
    st.get_param(id) > 0.5
}

fn default_norm(st: &Ctx, id: u32) -> Option<f64> {
    st.params().param_infos().iter().find(|i| i.id == id).map(|i| i.range.normalize(i.default_plain))
}

fn led(p: &egui::Painter, c: Pos2, on: f32, col: Color32) {
    let on = on.clamp(0.0, 1.0);
    if on > 0.02 {
        p.circle_filled(c, 8.0, alpha(col, 0.16 * on));
    }
    p.circle_filled(c, 3.6, lerp_color(LED_OFF, col, on));
}

fn current(st: &Ctx) -> (&'static Layout, usize, bool) {
    let sc = &st.params().scope;
    let alg = (sc.read(Slot::Algorithm) as usize).min(20);
    let quad = sc.read(Slot::Quad) > 0.5;
    let lay = if alg >= 12 {
        &PULSAR
    } else if quad {
        &QUAD[alg]
    } else {
        &MAIN[alg]
    };
    (lay, alg, quad)
}

// ===========================================================================
// Knobs
// ===========================================================================

#[allow(clippy::too_many_arguments)]
fn knob(ui: &mut egui::Ui, st: &Ctx, id: P, c: Pos2, r: f32, accent: Color32, marks: &[(f32, &str)], enabled: bool) -> bool {
    let raw: u32 = id.into();
    let hit = Rect::from_center_size(c, Vec2::splat(r * 2.0 + 16.0));
    let resp = ui.interact(hit, egui::Id::new(("knob", raw)), Sense::click_and_drag());
    let mut value = st.get_param(raw);
    if resp.double_clicked() {
        if let Some(d) = default_norm(st, raw) {
            set(st, id, d);
            value = d as f32;
        }
    } else {
        if resp.drag_started() {
            st.begin_edit(raw);
        }
        if resp.dragged() {
            let fine = ui.input(|i| i.modifiers.shift);
            value = (value - resp.drag_delta().y / if fine { 1000.0 } else { 240.0 }).clamp(0.0, 1.0);
            st.set_param(raw, f64::from(value));
        }
        if resp.drag_stopped() {
            st.end_edit(raw);
        }
        if resp.hovered() && !resp.dragged() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll.abs() > 0.0 {
                value = (value + scroll / 1500.0).clamp(0.0, 1.0);
                set(st, id, f64::from(value));
            }
        }
    }

    let p = ui.painter();
    let start = PI * 0.75;
    let sweep = PI * 1.5;
    let accent = if enabled { accent } else { FAINT };
    for i in 0..7 {
        let a = start + sweep * i as f32 / 6.0;
        p.circle_filled(polar(c, r + 8.0, a), if i == 0 || i == 6 { 1.5 } else { 1.1 }, alpha(TEXT, if enabled { 0.65 } else { 0.25 }));
    }
    for &(v, t) in marks {
        let a = start + sweep * v;
        p.text(polar(c, r + 19.0, a), Align2::CENTER_CENTER, t, FontId::monospace(8.5), DIM);
    }
    let active = resp.hovered() || resp.dragged();
    p.add(egui::Shape::line(arc(c, r + 3.5, start, sweep, 48), stroke(1.5, alpha(FAINT, 0.6))));
    if value > 0.003 {
        let n = ((value * 48.0) as usize).max(2);
        p.add(egui::Shape::line(arc(c, r + 3.5, start, sweep * value, n), stroke(2.0, accent)));
    }
    p.circle_filled(c + vec2(0.0, 2.0), r, Color32::from_black_alpha(140));
    let cap = if enabled { CAP } else { lerp_color(CAP, PANEL, 0.55) };
    p.circle_filled(c, r, cap);
    p.circle_stroke(c, r, stroke(1.0, CAP_SHADE));
    p.circle_stroke(c, r * 0.72, stroke(1.0, alpha(CAP_SHADE, 0.45)));
    let a = start + sweep * value;
    p.line_segment([polar(c, r * 0.18, a), polar(c, r * 0.92, a)], stroke(2.4, Color32::from_rgb(20, 20, 24)));
    if active {
        p.text(c + vec2(0.0, r + 13.0), Align2::CENTER_TOP, st.format_param(raw), FontId::proportional(10.5), accent);
    }
    active
}

/// A big knob with its name and a two-line hint beside it.
#[allow(clippy::too_many_arguments)]
fn labelled(ui: &mut egui::Ui, st: &Ctx, id: P, name: &str, hint: (&str, &str), c: Pos2, accent: Color32, marks: &[(f32, &str)], enabled: bool) {
    knob(ui, st, id, c, 29.0, accent, marks, enabled);
    let p = ui.painter();
    let tx = c.x + 50.0;
    p.text(pos2(tx, c.y - 14.0), Align2::LEFT_CENTER, name, FontId::proportional(14.0), if enabled { TEXT } else { FAINT });
    p.text(pos2(tx, c.y + 4.0), Align2::LEFT_CENTER, hint.0, FontId::proportional(9.0), DIM);
    p.text(pos2(tx, c.y + 16.0), Align2::LEFT_CENTER, hint.1, FontId::proportional(9.0), DIM);
}

fn small(ui: &mut egui::Ui, st: &Ctx, id: P, name: &str, c: Pos2, accent: Color32, marks: &[(f32, &str)]) {
    let active = knob(ui, st, id, c, 16.0, accent, marks, true);
    if !active {
        ui.painter().text(c + vec2(0.0, 30.0), Align2::CENTER_CENTER, name, FontId::proportional(10.0), TEXT);
    }
}

// ===========================================================================
// Header
// ===========================================================================

fn header(ui: &mut egui::Ui, st: &Ctx) {
    let p = ui.painter();
    p.line_segment([pos2(14.0, 52.0), pos2(W - 14.0, 52.0)], stroke(1.0, RULE));
    let g = pos2(34.0, 27.0);
    p.circle_stroke(g, 12.0, stroke(1.0, alpha(CYAN, 0.7)));
    p.add(egui::Shape::line(
        (0..=40)
            .map(|i| {
                let a = i as f32 / 40.0 * TAU;
                g + vec2(a.cos() * 15.0, a.sin() * 5.5)
            })
            .collect(),
        stroke(1.0, alpha(AMBER, 0.7)),
    ));
    p.circle_filled(g, 2.6, TEXT);
    p.text(pos2(58.0, 27.0), Align2::LEFT_CENTER, "S I D E R A L", FontId::proportional(23.0), TEXT);
    p.text(pos2(W - 16.0, 27.0), Align2::RIGHT_CENTER, "JARDIM SONORO", FontId::proportional(10.0), FAINT);

    let r = Rect::from_min_size(pos2(W - 300.0, 15.0), vec2(150.0, 24.0));
    p.text(pos2(r.left() - 10.0, r.center().y), Align2::RIGHT_CENTER, "PRIMES", FontId::proportional(9.0), FAINT);
    segmented(ui, st, P::Constellation, &["I", "II"], r, CYAN, true);
}

fn segmented(ui: &mut egui::Ui, st: &Ctx, id: P, labels: &[&str], r: Rect, accent: Color32, enabled: bool) {
    let n = labels.len();
    let idx = enum_index(st, id);
    let w = r.width() / n as f32;
    let raw: u32 = id.into();
    for (i, label) in labels.iter().enumerate() {
        let cell = Rect::from_min_size(pos2(r.left() + i as f32 * w, r.top()), vec2(w, r.height()));
        let resp = ui.interact(cell, egui::Id::new(("seg", raw, i)), Sense::click());
        if resp.clicked() {
            set_enum(st, id, i);
        }
        let on = i == idx;
        let acc = if enabled { accent } else { FAINT };
        let p = ui.painter();
        p.rect_filled(cell.shrink(1.5), CornerRadius::same(5), if on { alpha(acc, 0.85) } else { Color32::from_rgb(22, 25, 37) });
        if resp.hovered() && !on {
            p.rect_stroke(cell.shrink(1.5), CornerRadius::same(5), stroke(1.0, FAINT), StrokeKind::Inside);
        }
        p.text(cell.center(), Align2::CENTER_CENTER, *label, FontId::proportional(9.5), if on { BG } else { DIM });
    }
}

// ===========================================================================
// Side columns
// ===========================================================================

fn matter(ui: &mut egui::Ui, st: &Ctx, r: Rect) {
    section(ui.painter(), r, "MATTER");
    let (lay, alg, _) = current(st);
    let nova = (18..=19).contains(&alg);
    let kepler = alg == 20;
    let scatter = match lay.family {
        Family::TwoDelays | Family::FourDelays => ("lines bleed into", "each other"),
        Family::Reverb => ("— at rest in", "the nebula —"),
        Family::Granular => ("grains scattered", "by noise"),
        Family::Pulsar => ("the Halo:", "a small plate reverb"),
    };
    let drift = match lay.family {
        Family::TwoDelays | Family::FourDelays => ("lines wander across", "the sky, chaotically"),
        Family::Reverb => ("— at rest in", "the nebula —"),
        Family::Granular => ("grains wander", "on chaotic tides"),
        Family::Pulsar if kepler => ("down · still · up", "an octave, recursively"),
        Family::Pulsar if nova => ("past noon: random", "octave falls"),
        Family::Pulsar => ("the loops wander", "across the sky"),
    };
    let x = r.left() + 54.0;
    let rest = lay.family == Family::Reverb;
    labelled(ui, st, P::Blur, "SCATTER", scatter, pos2(x, r.top() + 80.0), CYAN, &[], !rest);
    labelled(ui, st, P::Drift, "DRIFT", drift, pos2(x, r.top() + 190.0), CYAN, &[], !rest);
    labelled(ui, st, P::Drive, "FLARE", ("odd harmonics", "burning in the loop"), pos2(x, r.top() + 300.0), AMBER, &[], true);

    let sc = &st.params().scope;
    let y = r.top() + 396.0;
    small(ui, st, P::Input, "IN", pos2(r.left() + 52.0, y), GREEN, &[]);
    let lvl = sc.read(Slot::InputPeak);
    led(ui.painter(), pos2(r.left() + 18.0, y), if lvl > 0.3 { 1.0 } else { lvl / 0.3 }, if lvl > 0.95 { RED } else { GREEN });
    small(ui, st, P::Clock, "DILATION", pos2(r.left() + 170.0, y), VIOLET, &[]);
}

fn gravity(ui: &mut egui::Ui, st: &Ctx, r: Rect) {
    section(ui.painter(), r, "GRAVITY");
    let sc = &st.params().scope;
    let x = r.left() + 54.0;
    let tide = st.get_param(P::Supcom);
    let tide_hint = if tide < 0.49 {
        ("ebb: each new sound", "washes the old away")
    } else if tide > 0.51 {
        ("flow: soft rises,", "loud settles")
    } else {
        ("ebb  ·  still  ·  flow", "")
    };
    labelled(ui, st, P::Supcom, "TIDE", tide_hint, pos2(x, r.top() + 80.0), VIOLET, &[(0.0, "ebb"), (1.0, "flow")], true);
    let fbv = crate::engine::fb_gain(st.get_param(P::Feedback));
    let orbit_hint = if fbv > 1.0005 {
        "escaping: it grows"
    } else if fbv > 0.995 {
        "a stable orbit"
    } else {
        "a decaying orbit"
    };
    let gain_txt = format!("×{fbv:.3}");
    labelled(ui, st, P::Feedback, "ORBIT", (&gain_txt, orbit_hint), pos2(x, r.top() + 190.0), AMBER, &[(0.162, "0.3"), (0.453, "0.7"), (0.952, "1")], true);
    let mix = st.get_param(P::Mix);
    let mix_txt = format!("{:.0}% memory", mix * 100.0);
    labelled(ui, st, P::Mix, "MIX", (&mix_txt, "against the present"), pos2(x, r.top() + 300.0), CYAN, &[(0.5, "·")], true);

    let p = ui.painter();
    let mx = r.left() + 104.0;
    meter(p, pos2(mx, r.top() + 106.0), 130.0, sc.read(Slot::Suppress), VIOLET);
    meter(p, pos2(mx, r.top() + 216.0), 130.0, sc.read(Slot::Limiter) * 4.0, RED);

    let y = r.top() + 396.0;
    small(ui, st, P::Output, "OUT", pos2(r.left() + 52.0, y), GREEN, &[]);
    let out = sc.read(Slot::OutputPeak);
    led(ui.painter(), pos2(r.left() + 18.0, y), (out / 0.3).min(1.0), if out > 0.95 { RED } else { GREEN });
    small(ui, st, P::Width, "PARALLAX", pos2(r.left() + 170.0, y), CYAN, &[]);
}

fn meter(p: &egui::Painter, at: Pos2, w: f32, v: f32, col: Color32) {
    let r = Rect::from_min_size(at, vec2(w, 3.0));
    p.rect_filled(r, CornerRadius::same(2), LED_OFF);
    p.rect_filled(Rect::from_min_size(at, vec2(w * v.clamp(0.0, 1.0), 3.0)), CornerRadius::same(2), col);
}

// ===========================================================================
// The orrery and the program constellations
// ===========================================================================

fn dial_angle(i: usize) -> f32 {
    let start = PI * (2.0 / 3.0);
    let span = PI * (5.0 / 3.0);
    let gap = 0.12;
    let step = (span - 4.0 * gap) / 20.0;
    let fam = FAMILIES.iter().position(|f| i >= f.1 && i < f.1 + f.2).unwrap_or(0);
    start + step * i as f32 + gap * fam as f32
}

fn orrery(ui: &mut egui::Ui, st: &Ctx, c: Pos2, t: f32) {
    let sc = &st.params().scope;
    let (lay, active, _) = current(st);
    let selected = enum_index(st, P::Program);
    let p = ui.painter().clone();
    let eclipse = sc.read(Slot::Eclipse) > 0.5;
    let redshift = sc.read(Slot::Redshift) > 0.5;
    let dust = is_on(st, P::Grit);
    let shade = |col: Color32| -> Color32 {
        let col = if redshift { lerp_color(col, RED, 0.5) } else { col };
        if eclipse { lerp_color(col, Color32::from_gray(90), 0.55) } else { col }
    };

    // --- the memory ring ---------------------------------------------------
    let r_mem = 156.0;
    let loop_len = sc.read(Slot::LoopLen) as usize;
    let seg_of = |idx: usize| -> Option<usize> {
        if lay.family == Family::Pulsar {
            if idx < loop_len {
                return Some(0);
            }
            if idx >= RB && idx < RB + loop_len {
                return Some(1);
            }
            return None;
        }
        let mut b = 0;
        for (k, &n) in lay.lines.iter().enumerate() {
            if idx < b + n {
                return Some(k);
            }
            b += n;
        }
        None
    };
    p.circle_stroke(c, r_mem, stroke(1.0, alpha(FAINT, 0.5)));
    let per = MEM / BINS;
    for b in 0..BINS {
        let a = -PI / 2.0 + TAU * (b as f32 + 0.5) / BINS as f32;
        let v = sc.bin(b).sqrt().min(1.0);
        let col = seg_of(b * per).map_or(Color32::from_gray(90), |k| shade(ORBIT[k % 9]));
        let len = 2.0 + v * 26.0;
        p.line_segment([polar(c, r_mem + 1.0, a), polar(c, r_mem + 1.0 + len, a)], stroke(1.6, alpha(col, 0.22 + 0.7 * v)));
    }
    if lay.family != Family::Pulsar {
        let mut b = 0;
        for &n in lay.lines.iter() {
            let a = -PI / 2.0 + TAU * b as f32 / MEM as f32;
            p.line_segment([polar(c, r_mem - 5.0, a), polar(c, r_mem + 5.0, a)], stroke(1.0, alpha(TEXT, 0.55)));
            b += n;
        }
    }
    if dust {
        for i in 0..90u32 {
            let k = i + (t * 24.0) as u32 * 97;
            let a = hash(k) * TAU;
            let rr = 30.0 + hash(k + 1) * 160.0;
            p.circle_filled(polar(c, rr, a), 0.9, alpha(SAND, 0.25 + 0.5 * hash(k + 2)));
        }
    }

    // --- orbits --------------------------------------------------------------
    let level = sc.read(Slot::MemoryPeak).min(1.0);
    let head = |s: usize| sc.get(Slot::Head0 as usize + s);
    let reverse = is_on(st, P::Reverse);
    let planet = |r: f32, a: f32, col: Color32, size: f32| {
        let dir = if reverse { 1.0 } else { -1.0 };
        for k in 0..14 {
            let a0 = a + dir * k as f32 * 0.035;
            let a1 = a + dir * (k + 1) as f32 * 0.035;
            p.line_segment([polar(c, r, a0), polar(c, r, a1)], stroke(2.0, alpha(col, 0.5 * (1.0 - k as f32 / 14.0))));
        }
        p.circle_filled(polar(c, r, a), size + 4.0, alpha(col, 0.15 + 0.2 * level));
        p.circle_filled(polar(c, r, a), size, col);
    };

    if lay.family == Family::Pulsar {
        pulsar_orbits(&p, st, c, active, t, &shade, &planet);
    } else {
        let max_len = 1_097_101.0f32;
        let radius = |n: usize| 32.0 + 100.0 * (n as f32 / max_len).sqrt();
        let slots: Vec<(usize, usize)> = match lay.family {
            Family::TwoDelays => vec![(0, 0), (1, 1)],
            Family::FourDelays => vec![(0, 0), (1, 1), (2, 3), (3, 4)],
            Family::Reverb => (0..8).map(|k| (k, k)).collect(),
            _ => vec![(0, 0)],
        };
        for &(line, slot) in &slots {
            let n = lay.lines[line];
            let r = radius(n);
            let col = shade(ORBIT[line % 9]);
            let thin = lay.family == Family::Reverb && line % 2 == 1;
            p.circle_stroke(c, r, stroke(if thin { 0.8 } else { 1.0 }, alpha(col, 0.22)));
            planet(r, -PI / 2.0 + TAU * head(slot), col, if thin { 2.6 } else { 3.6 + level * 3.0 });
        }
        if lay.family == Family::Granular {
            let r = radius(lay.lines[0]);
            for k in 0..12 {
                if k >= 8 && lay.center.is_none() {
                    break;
                }
                let g = sc.get(Slot::Grain0 as usize + k);
                let (rr, col) = match k / 4 {
                    0 => (r - 9.0, CYAN),
                    1 => (r + 9.0, AMBER),
                    _ => (r, VIOLET),
                };
                let a = -PI / 2.0 + TAU * g;
                p.circle_filled(polar(c, rr, a), 2.2, alpha(shade(col), 0.9));
            }
        }
    }

    // --- the sun, the LFOs, the eclipse ---------------------------------------
    let lvl = sc.read(Slot::InputPeak).min(1.0);
    p.circle_filled(c, 16.0 + lvl * 10.0, alpha(AMBER, 0.06 + 0.12 * lvl));
    p.circle_filled(c, 7.0 + lvl * 3.0, lerp_color(Color32::from_rgb(120, 90, 40), AMBER, 0.4 + lvl));
    if eclipse {
        let pulse = 0.5 + 0.5 * (t * 1.3).sin();
        for k in 0..3 {
            p.circle_stroke(c, 13.0 + k as f32 * 5.0 + pulse * 2.0, stroke(1.0, alpha(Color32::WHITE, 0.25 - k as f32 * 0.07)));
        }
        p.circle_filled(c + vec2(1.5, -1.0), 11.0, Color32::from_rgb(4, 4, 8));
    }
    if matches!(lay.family, Family::TwoDelays | Family::FourDelays | Family::Granular | Family::Pulsar) {
        let n = if matches!(lay.family, Family::FourDelays | Family::Granular) { 4 } else { 2 };
        for k in 0..n {
            let v = sc.get(Slot::Lfo0 as usize + k).clamp(0.0, 1.0);
            let a = -PI / 2.0 + k as f32 * TAU / n as f32 + (v - 0.5) * 1.2;
            p.circle_filled(polar(c, 22.0, a), 1.6, alpha(TEXT, 0.7));
        }
    }

    // --- the five constellations of programs -------------------------------------
    let r_dial = 206.0;
    let settle = sc.read(Slot::Settle);
    let switching = sc.read(Slot::Switching) > 0.5;
    for &(name, first, count) in FAMILIES.iter() {
        let a0 = dial_angle(first) - 0.05;
        let a1 = dial_angle(first + count - 1) + 0.05;
        let on = selected >= first && selected < first + count;
        let col = if on { TEXT } else { FAINT };
        p.add(egui::Shape::line(arc(c, r_dial + 12.0, a0, a1 - a0, 24), stroke(1.0, alpha(col, 0.8))));
        let mid = (a0 + a1) * 0.5;
        let lp = polar(c, r_dial + 26.0, mid);
        let align = if mid.cos() > 0.3 {
            Align2::LEFT_CENTER
        } else if mid.cos() < -0.3 {
            Align2::RIGHT_CENTER
        } else {
            Align2::CENTER_CENTER
        };
        p.text(lp, align, name, FontId::proportional(9.0), if on { TEXT } else { DIM });
    }
    for i in 0..21 {
        let a = dial_angle(i);
        let pos = polar(c, r_dial, a);
        let hit = Rect::from_center_size(pos, Vec2::splat(18.0));
        let resp = ui.interact(hit, egui::Id::new(("prog", i)), Sense::click());
        if resp.clicked() {
            set_enum(st, P::Program, i);
        }
        let col = if i == active {
            TEXT
        } else if i == selected {
            AMBER
        } else if resp.hovered() {
            DIM
        } else {
            FAINT
        };
        if i < 12 {
            p.circle_filled(pos, [2.8, 4.2, 5.8][i % 3], col);
        } else {
            // Pulsars: small four-pointed stars.
            let s = 4.5;
            p.line_segment([pos - vec2(s, 0.0), pos + vec2(s, 0.0)], stroke(1.4, col));
            p.line_segment([pos - vec2(0.0, s), pos + vec2(0.0, s)], stroke(1.4, col));
            p.circle_filled(pos, 1.8, col);
        }
        if i == selected && switching {
            p.add(egui::Shape::line(arc(pos, 9.0, -PI / 2.0, TAU * settle, 24), stroke(1.5, AMBER)));
        }
        if resp.hovered() {
            p.text(polar(c, r_dial - 18.0, a), Align2::CENTER_CENTER, NAMES[i], FontId::proportional(9.5), TEXT);
        }
    }
    let a = dial_angle(selected);
    p.line_segment([polar(c, r_dial - 13.0, a), polar(c, r_dial - 6.0, a)], stroke(2.0, AMBER));

    // Drag on the outer sky to sweep through the programs.
    let ring = Rect::from_center_size(c, Vec2::splat(r_dial * 2.0 + 30.0));
    let resp = ui.interact(ring, egui::Id::new("dial-drag"), Sense::drag());
    if resp.dragged()
        && let Some(pp) = resp.interact_pointer_pos()
    {
        let d = pp - c;
        if d.length() > r_dial - 26.0 {
            let mut a = d.y.atan2(d.x);
            if a < PI * 0.5 {
                a += TAU;
            }
            let best = (0..21).min_by(|&x, &y| (dial_angle(x) - a).abs().total_cmp(&(dial_angle(y) - a).abs())).unwrap_or(selected);
            if best != selected {
                set_enum(st, P::Program, best);
            }
        }
    }
}

fn pulsar_orbits(
    p: &egui::Painter,
    st: &Ctx,
    c: Pos2,
    alg: usize,
    t: f32,
    shade: &dyn Fn(Color32) -> Color32,
    planet: &dyn Fn(f32, f32, Color32, f32),
) {
    let sc = &st.params().scope;
    let state = sc.read(Slot::LoopState) as u8;
    let len = sc.read(Slot::LoopLen) as usize;
    let (rl, rr) = (112.0, 82.0);
    let (cl, cr) = (shade(ORBIT[0]), shade(ORBIT[1]));
    let level = sc.read(Slot::MemoryPeak).min(1.0);
    match state {
        0 => {
            // Empty: dashed orbits waiting for matter.
            for (r, col) in [(rl, cl), (rr, cr)] {
                for k in 0..48 {
                    let a = k as f32 / 48.0 * TAU + t * 0.05;
                    p.add(egui::Shape::line(arc(c, r, a, 0.06, 3), stroke(1.0, alpha(col, 0.35))));
                }
            }
            p.text(c + vec2(0.0, 46.0), Align2::CENTER_CENTER, "awaiting matter · ACCRETE", FontId::proportional(10.0), DIM);
            return;
        }
        1 => {
            let f = len as f32 / MAX_LOOP as f32;
            for (r, col) in [(rl, cl), (rr, cr)] {
                p.circle_stroke(c, r, stroke(1.0, alpha(col, 0.15)));
                p.add(egui::Shape::line(arc(c, r, -PI / 2.0, TAU * f, 96), stroke(2.4, col)));
            }
            p.text(c + vec2(0.0, 46.0), Align2::CENTER_CENTER, "accreting · press ACCRETE again to close the orbit", FontId::proportional(10.0), AMBER);
            return;
        }
        _ => {}
    }
    let a_l = -PI / 2.0 + TAU * sc.get(Slot::Head0 as usize);
    let a_r = -PI / 2.0 + TAU * sc.get(Slot::Head0 as usize + 1);
    p.circle_stroke(c, rl, stroke(1.0, alpha(cl, 0.25)));
    p.circle_stroke(c, rr, stroke(1.0, alpha(cr, 0.25)));

    let slices = sc.read(Slot::Slices) as usize;
    if slices > 1 {
        let step = TAU / slices as f32;
        for (r, col, src, a_now) in [
            (rl, cl, sc.read(Slot::SliceSrcL) as usize, a_l),
            (rr, cr, sc.read(Slot::SliceSrcR) as usize, a_r),
        ] {
            for s in 0..slices {
                let a = -PI / 2.0 + step * s as f32;
                p.line_segment([polar(c, r - 6.0, a), polar(c, r + 6.0, a)], stroke(1.0, alpha(col, 0.5)));
            }
            // The torn-off slice the head is actually playing from.
            let q = sc.read(Slot::SliceQ);
            let a_src = -PI / 2.0 + step * (src as f32 + q);
            p.add(egui::Shape::line(arc(c, r, -PI / 2.0 + step * src as f32, step, 24), stroke(3.0, alpha(col, 0.55))));
            // A tidal thread from where we are to where the sound comes from.
            let n = 24;
            let pts: Vec<Pos2> = (0..=n)
                .map(|i| {
                    let u = i as f32 / n as f32;
                    let a = a_now + (a_src - a_now) * u;
                    polar(c, r * (1.0 - 0.45 * (u * PI).sin()), a)
                })
                .collect();
            p.add(egui::Shape::line(pts, stroke(1.0, alpha(col, 0.45))));
            p.circle_filled(polar(c, r, a_src), 3.0, col);
        }
        let flash = sc.read(Slot::Flash);
        if (18..=19).contains(&alg) && flash > 0.02 {
            p.circle_stroke(c, 128.0 + (1.0 - flash) * 30.0, stroke(2.0, alpha(ROSE, flash * 0.8)));
        }
    }
    if alg == 20 {
        // Kepler: the harmonic ghost, an octave away.
        let d = st.get_param(P::Drift);
        let ghost = if d >= 0.75 {
            Some(2.0)
        } else if d < 0.25 {
            Some(0.5)
        } else {
            None
        };
        if let Some(m) = ghost {
            let ph = sc.get(Slot::Head0 as usize);
            let a = -PI / 2.0 + TAU * (ph * m).fract();
            p.circle_filled(polar(c, rl - 16.0, a), 3.0, alpha(VIOLET, 0.9));
            p.text(c + vec2(0.0, 46.0), Align2::CENTER_CENTER, if m > 1.0 { "octave up, compounding" } else { "octave down, compounding" }, FontId::proportional(9.5), VIOLET);
        }
    }
    if (13..=14).contains(&alg) {
        p.text(c + vec2(0.0, 46.0), Align2::CENTER_CENTER, if alg == 13 { "8 : 7" } else { "4 : 3" }, FontId::proportional(12.0), DIM);
    }
    planet(rl, a_l, cl, 3.8 + level * 3.0);
    planet(rr, a_r, cr, 3.8 + level * 3.0);
}

fn status_line(ui: &mut egui::Ui, st: &Ctx, r: Rect) {
    let sc = &st.params().scope;
    let (lay, active, quad) = current(st);
    let p = ui.painter();
    let speed = {
        let v = sc.read(Slot::Speed);
        if v < 0.4 { 1.0 } else { v }
    };
    let secs = |n: usize| n as f64 / (HW_RATE * f64::from(speed));
    let mut lines = String::new();
    match lay.family {
        Family::Pulsar => {
            let len = sc.read(Slot::LoopLen) as usize;
            match sc.read(Slot::LoopState) as u8 {
                0 => lines.push_str("no orbit yet"),
                1 => lines.push_str(&format!("accreting  {:.2}s", secs(len))),
                _ => lines.push_str(&format!("period {:.3}s", secs(len))),
            }
            let period = enum_index(st, P::Period);
            if period > 0 {
                lines.push_str(&format!("  ·  locked to {} bar{}", [0, 1, 2, 4, 8][period], if period > 1 { "s" } else { "" }));
            }
        }
        _ => {
            for (k, &n) in lay.lines.iter().enumerate() {
                if lay.family == Family::Reverb && k % 2 == 1 {
                    continue;
                }
                lines.push_str(&format!("{:.2}s  ", secs(n)));
            }
            if let Some(g) = lay.grains {
                lines.push_str(&format!("· grain {:.0}ms", secs(g.len as usize) * 1000.0));
            }
        }
    }
    let title = if sc.read(Slot::Clearing) > 0.5 {
        "big crunch…".to_owned()
    } else if sc.read(Slot::Switching) > 0.5 {
        format!("{}  →  {}", NAMES[active], NAMES[enum_index(st, P::Program)])
    } else {
        format!("{}{}", NAMES[active], if quad && active < 12 { "   ·   primes II" } else { "" })
    };
    p.text(pos2(r.center().x, r.top() + 4.0), Align2::CENTER_TOP, title, FontId::proportional(14.0), TEXT);
    p.text(pos2(r.center().x, r.top() + 23.0), Align2::CENTER_TOP, lines, FontId::monospace(9.5), DIM);

    let m = sc.read(Slot::MemoryPeak);
    for (k, &tv) in [0.07, 0.155, 0.31, 0.56, 0.9].iter().enumerate() {
        let col = match k {
            0..=2 => GREEN,
            3 => AMBER,
            _ => RED,
        };
        led(p, pos2(r.left() + 6.0 + k as f32 * 12.0, r.top() + 14.0), if m >= tv { 1.0 } else { (m / tv).powi(3) * 0.4 }, col);
    }
}

// ===========================================================================
// Performance pads
// ===========================================================================

#[derive(Clone, Copy)]
enum Icon {
    Accrete,
    Erode,
    Retrograde,
    Eclipse,
    Redshift,
    Dust,
    Crunch,
}

fn icon(p: &egui::Painter, c: Pos2, kind: Icon, col: Color32, t: f32, on: bool) {
    let s = stroke(1.6, col);
    match kind {
        Icon::Accrete => {
            // An accretion disc spiralling into a core.
            let pts: Vec<Pos2> = (0..=60)
                .map(|i| {
                    let u = i as f32 / 60.0;
                    let a = u * TAU * 2.2 + if on { t * 2.0 } else { 0.0 };
                    c + vec2(a.cos() * (14.0 - 11.0 * u), a.sin() * (14.0 - 11.0 * u) * 0.55)
                })
                .collect();
            p.add(egui::Shape::line(pts, s));
            p.circle_filled(c, 3.0, col);
        }
        Icon::Erode => {
            for k in 0..10 {
                let a = k as f32 / 10.0 * TAU;
                let r = if k % 3 == 0 { 9.0 } else { 12.0 };
                p.add(egui::Shape::line(arc(c, r, a, 0.35, 4), s));
            }
        }
        Icon::Retrograde => {
            p.add(egui::Shape::line(arc(c, 11.0, -PI * 0.2, -PI * 1.5, 30), s));
            let tip = polar(c, 11.0, -PI * 1.7);
            p.line_segment([tip, tip + vec2(7.0, -1.0)], s);
            p.line_segment([tip, tip + vec2(1.0, 7.0)], s);
        }
        Icon::Eclipse => {
            p.circle_filled(c, 11.0, col);
            p.circle_filled(c + vec2(5.0, -3.0), 10.5, PAD);
        }
        Icon::Redshift => {
            let pts: Vec<Pos2> = (0..=40)
                .map(|i| {
                    let u = i as f32 / 40.0;
                    let ph = (1.0 - (1.0 - u) * (1.0 - u)) * 14.0;
                    c + vec2(-15.0 + 30.0 * u, -(ph + if on { t * 4.0 } else { 0.0 }).sin() * 7.0)
                })
                .collect();
            p.add(egui::Shape::line(pts, s));
        }
        Icon::Dust => {
            for k in 0..16u32 {
                let k2 = k + if on { (t * 8.0) as u32 * 31 } else { 0 };
                let o = vec2(hash(k2 * 2) - 0.5, hash(k2 * 2 + 1) - 0.5) * 28.0;
                p.circle_filled(c + o, 1.2, col);
            }
        }
        Icon::Crunch => {
            for k in 0..8 {
                let a = k as f32 / 8.0 * TAU;
                p.line_segment([polar(c, 14.0, a), polar(c, 5.0, a)], s);
            }
            p.circle_filled(c, 2.2, col);
        }
    }
}

/// A flat, luminous pad.
#[allow(clippy::too_many_arguments)]
fn pad(ui: &mut egui::Ui, key: &str, r: Rect, on: f32, col: Color32, title: &str, sub: &str, kind: Icon, t: f32, enabled: bool) -> egui::Response {
    let resp = ui.interact(r, egui::Id::new(("pad", key)), Sense::click_and_drag());
    let p = ui.painter();
    let on = on.clamp(0.0, 1.0);
    let col = if enabled { col } else { FAINT };
    if on > 0.01 {
        p.rect_filled(r.expand(4.0), CornerRadius::same(14), alpha(col, 0.07 * on));
    }
    p.rect_filled(r, CornerRadius::same(12), lerp_color(PAD, lerp_color(PAD, col, 0.22), on));
    let border = if resp.hovered() { alpha(col, 0.8) } else { alpha(col, 0.2 + 0.6 * on) };
    p.rect_stroke(r, CornerRadius::same(12), stroke(1.0, border), StrokeKind::Inside);
    if resp.is_pointer_button_down_on() {
        p.rect_filled(r, CornerRadius::same(12), alpha(Color32::WHITE, 0.05));
    }
    let ic = pos2(r.center().x, r.top() + 30.0);
    icon(p, ic, kind, if on > 0.5 { col } else { lerp_color(DIM, col, 0.35) }, t, on > 0.5);
    p.text(pos2(r.center().x, r.top() + 60.0), Align2::CENTER_CENTER, title, FontId::proportional(12.5), if on > 0.5 { TEXT } else { lerp_color(DIM, TEXT, 0.5) });
    if !sub.is_empty() {
        p.text(pos2(r.center().x, r.top() + 79.0), Align2::CENTER_CENTER, sub, FontId::proportional(8.5), if enabled { DIM } else { FAINT });
    }
    resp
}

fn pads(ui: &mut egui::Ui, st: &Ctx, s: &mut SideralUi, r: Rect, t: f32) {
    let n = 7.0;
    let gap = 10.0;
    let w = (r.width() - gap * (n - 1.0)) / n;
    let cell = |i: usize| Rect::from_min_size(pos2(r.left() + i as f32 * (w + gap), r.top()), vec2(w, r.height()));
    let (lay, _, _) = current(st);
    let sc = &st.params().scope;

    // ACCRETE: record, with its two groups.
    let rec = enum_index(st, P::Record);
    if rec != 0 {
        s.last_rec = rec;
    }
    let rc = cell(0);
    let chips_r = Rect::from_min_size(pos2(rc.left() + 14.0, rc.bottom() - 24.0), vec2(rc.width() - 28.0, 16.0));
    let chip_hit = ui.rect_contains_pointer(chips_r);
    let resp = pad(ui, "accrete", rc, if rec != 0 { 1.0 } else { 0.0 }, AMBER, "ACCRETE", "", Icon::Accrete, t, true);
    if resp.clicked() && !chip_hit {
        set_enum(st, P::Record, if rec == 0 { s.last_rec } else { 0 });
    }
    for (i, l) in ["L", "R", "L+R"].iter().enumerate() {
        let cw = chips_r.width() / 3.0;
        let cr = Rect::from_min_size(pos2(chips_r.left() + i as f32 * cw, chips_r.top()), vec2(cw, chips_r.height())).shrink(1.0);
        let cresp = ui.interact(cr, egui::Id::new(("recchip", i)), Sense::click());
        if cresp.clicked() {
            s.last_rec = i + 1;
            set_enum(st, P::Record, i + 1);
        }
        let sel = s.last_rec == i + 1;
        let p = ui.painter();
        let fill = if sel && rec != 0 {
            alpha(AMBER, 0.85)
        } else if sel {
            alpha(AMBER, 0.25)
        } else {
            Color32::from_rgb(26, 29, 42)
        };
        p.rect_filled(cr, CornerRadius::same(4), fill);
        p.text(cr.center(), Align2::CENTER_CENTER, *l, FontId::proportional(8.5), if sel && rec != 0 { BG } else { DIM });
    }

    // ERODE: hold to wear away what is playing.
    let erode = is_on(st, P::Erase);
    let resp = pad(ui, "erode", cell(1), if erode { 1.0 } else { 0.0 }, ROSE, "ERODE", "hold to wear away", Icon::Erode, t, true);
    let held = resp.is_pointer_button_down_on();
    if held != erode {
        set(st, P::Erase, if held { 1.0 } else { 0.0 });
    }

    let retro = is_on(st, P::Reverse);
    if pad(ui, "retro", cell(2), if retro { 1.0 } else { 0.0 }, CYAN, "RETROGRADE", "the tape turns back", Icon::Retrograde, t, true).clicked() {
        set(st, P::Reverse, if retro { 0.0 } else { 1.0 });
    }

    let ecl = is_on(st, P::Eclipse);
    if pad(ui, "eclipse", cell(3), if ecl { 1.0 } else { 0.0 }, Color32::from_rgb(230, 230, 240), "ECLIPSE", "memory held, untouched", Icon::Eclipse, t, true).clicked() {
        set(st, P::Eclipse, if ecl { 0.0 } else { 1.0 });
    }

    let rs = is_on(st, P::HalfSpeed);
    let rs_ok = matches!(lay.family, Family::TwoDelays | Family::FourDelays | Family::Pulsar);
    let rs_sub = if rs_ok { "half speed, an octave down" } else { "binaries, trapezia, pulsars" };
    if pad(ui, "redshift", cell(4), if rs { 1.0 } else { 0.0 }, RED, "REDSHIFT", rs_sub, Icon::Redshift, t, rs_ok).clicked() {
        set(st, P::HalfSpeed, if rs { 0.0 } else { 1.0 });
    }

    let dust = is_on(st, P::Grit);
    if pad(ui, "dust", cell(5), if dust { 1.0 } else { 0.0 }, SAND, "DUST", "sample & hold grain", Icon::Dust, t, true).clicked() {
        set(st, P::Grit, if dust { 0.0 } else { 1.0 });
    }

    // BIG CRUNCH: hold for 0.8 s to collapse the whole memory.
    let now = ui.input(|i| i.time);
    let cr = cell(6);
    let clearing = sc.read(Slot::Clearing) > 0.5;
    let progress = s.crunch_start.map_or(0.0, |t0| ((now - t0) / 0.8) as f32);
    let glow = if clearing { 1.0 } else if progress.is_finite() { progress * 0.8 } else { 0.0 };
    let resp = pad(ui, "crunch", cr, glow, RED, "BIG CRUNCH", "hold to collapse it all", Icon::Crunch, t, true);
    if resp.is_pointer_button_down_on() {
        let t0 = *s.crunch_start.get_or_insert(now);
        if now - t0 >= 0.8 {
            sc.erase_all.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            s.crunch_start = Some(f64::INFINITY);
        }
    } else {
        s.crunch_start = None;
    }
    if progress > 0.0 && progress.is_finite() {
        ui.painter().add(egui::Shape::line(arc(pos2(cr.center().x, cr.top() + 30.0), 19.0, -PI / 2.0, TAU * progress.min(1.0), 40), stroke(2.0, RED)));
    }
}

// ===========================================================================
// The lower strip: tone, period, horizon
// ===========================================================================

fn strip(ui: &mut egui::Ui, st: &Ctx, r: Rect) {
    section(ui.painter(), r, "");
    let (lay, _, _) = current(st);
    let y = r.top() + 34.0;
    let h = 26.0;
    let caption = |ui: &mut egui::Ui, x: f32, text: &str, sub: &str| {
        let p = ui.painter();
        p.text(pos2(x + 2.0, r.top() + 18.0), Align2::LEFT_CENTER, text, FontId::proportional(10.0), TEXT);
        p.text(pos2(x + 2.0, r.top() + 76.0), Align2::LEFT_CENTER, sub, FontId::proportional(8.5), FAINT);
    };
    let mut x = r.left() + 16.0;
    caption(ui, x, "LOW CUT", "Hz · inside the orbit");
    segmented(ui, st, P::Hpf, &["OFF", "266", "462", "786"], Rect::from_min_size(pos2(x, y), vec2(220.0, h)), GREEN, true);
    x += 240.0;
    caption(ui, x, "HIGH CUT", "Hz · a tape-like bend");
    segmented(ui, st, P::Lpf, &["OFF", "622", "1.2k", "5.2k"], Rect::from_min_size(pos2(x, y), vec2(220.0, h)), GREEN, true);
    x += 240.0;
    let pulsar = lay.family == Family::Pulsar;
    caption(ui, x, "PERIOD", if pulsar { "pulsar orbit, locked to the host bars" } else { "pulsars only" });
    segmented(ui, st, P::Period, &["FREE", "1", "2", "4", "8"], Rect::from_min_size(pos2(x, y), vec2(250.0, h)), AMBER, pulsar);
    x += 270.0;
    caption(ui, x, "HORIZON", "below it, nothing enters");
    segmented(ui, st, P::Gate, &["OPEN", "−48", "−68"], Rect::from_min_size(pos2(x, y), vec2(190.0, h)), VIOLET, true);
    x += 210.0;
    caption(ui, x, "INPUT", "two sources, one sky");
    let tr = Rect::from_min_size(pos2(x, y), vec2(r.right() - x - 16.0, h));
    let resp = ui.interact(tr, egui::Id::new("mono"), Sense::click());
    let on = is_on(st, P::Mono);
    if resp.clicked() {
        set(st, P::Mono, if on { 0.0 } else { 1.0 });
    }
    let p = ui.painter();
    p.rect_filled(tr.shrink(1.5), CornerRadius::same(5), if on { alpha(CYAN, 0.85) } else { Color32::from_rgb(22, 25, 37) });
    p.text(tr.center(), Align2::CENTER_CENTER, if on { "SUMMED" } else { "STEREO" }, FontId::proportional(9.5), if on { BG } else { DIM });
}
