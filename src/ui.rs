//! Editor: the whole window is one sky. There are no knobs, pads or switches.
//!
//! Centre: an orrery. The outer ring is the whole 2^21-sample memory, its
//! envelope coloured by the segments of the active program; inside, every
//! delay line is an orbit whose planet is the write head. Around it, the
//! twenty-one programs as stars on a dial, their names curved along it.
//!
//! Left and right: ten bodies, each on its own orbit around the sun. Drag a
//! body along its orbit to change it (up is more); double-tap to send it
//! home. Each one looks like what it does: SCATTER is a nebula that spreads,
//! DRIFT a comet whose tail grows, TIDE and MIX are moons with phases.
//! Below the dial, in a row: REC, a red giant with its L and R moons, and
//! three phenomena either side of it to tap or hold. Along the horizon: six small constellations,
//! one star lit in each.
//!
//! Every target is at least 46 px across and nothing depends on hover, a
//! wheel or a modifier key, so it works under a finger as well as a mouse.

use crate::engine::layouts::{Family, HW_RATE, Layout, MAIN, MEM, NAMES, PULSAR, QUAD};
use crate::engine::pulsar::{MAX_LOOP, RB};
use crate::scope::{BINS, Slot};
use crate::{SideralParams, SideralParamsParamId as P};
use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2, pos2, vec2};
use std::f32::consts::{PI, TAU};
use std::sync::Arc;
use truce::prelude::*;
use truce_egui::EguiEditor;

const W: f32 = 1180.0;
const H: f32 = 790.0;
/// Scale of the orrery: every radius, planet and label in the inner sky.
const K: f32 = 1.1;
/// The sun.
const C: Pos2 = Pos2::new(590.0, 318.0);
const R_MEM: f32 = 156.0 * K;
const R_DIAL: f32 = 206.0 * K;
/// Orbits of the ten bodies: radius of the innermost, and the step outward.
const LANE0: f32 = 314.0;
const LANE_STEP: f32 = 54.0;
/// Half the angle each of those orbits sweeps, either side of the horizontal.
const SPAN: f32 = 28.0 * PI / 180.0;
/// The row under the dial: REC in the middle, three phenomena either side.
const ROW_Y: f32 = 636.0;
const ROW_STEP: f32 = 125.0;
/// Smallest side of anything meant to be touched.
const TOUCH: f32 = 46.0;

// --- Palette ------------------------------------------------------------------
const BG: Color32 = Color32::from_rgb(6, 7, 12);
const PANEL: Color32 = Color32::from_rgb(13, 15, 23);
const TEXT: Color32 = Color32::from_rgb(232, 229, 220);
const DIM: Color32 = Color32::from_rgb(125, 130, 150);
const FAINT: Color32 = Color32::from_rgb(66, 70, 90);
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

/// The five families, as the dial spaces them.
const FAMILIES: [(usize, usize); 5] = [(0, 3), (3, 3), (6, 3), (9, 3), (12, 9)];
/// The names written along the dial: the Pulsar family is spelled out.
const GROUPS: [(&str, usize, usize); 9] = [
    ("BINARY", 0, 3),
    ("TRAPEZIUM", 3, 3),
    ("NEBULA", 6, 3),
    ("STARDUST", 9, 3),
    ("PULSAR", 12, 1),
    ("RESONANCE", 13, 2),
    ("ROCHE", 15, 3),
    ("SUPERNOVA", 18, 2),
    ("KEPLER", 20, 1),
];

pub fn create_editor(params: Arc<SideralParams>) -> Box<dyn Editor> {
    // Each instance gets its own sky: the seed is where its scope happens to
    // live, so it holds while the plugin is loaded and differs on the next load.
    let seed = (Arc::as_ptr(&params.scope) as usize as u32 >> 4).wrapping_mul(0x9E37_79B9);
    let ui = SideralUi { seed, ..SideralUi::default() };
    Box::new(EguiEditor::with_ui(params, (W as u32, H as u32), ui).with_visuals(visuals()))
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
    /// Where on the body the pointer took hold, in value units.
    grab: f32,
    /// Phase of DILATION's moon; it turns at the speed of time.
    spin: f32,
    /// What the body being moved does, shown under the title on the next frame.
    hint: Option<(String, Color32)>,
    /// Scatters the background stars; fixed for the life of the instance.
    seed: u32,
}

impl Default for SideralUi {
    fn default() -> Self {
        Self { last_rec: 3, crunch_start: None, grab: 0.0, spin: 0.0, hint: None, seed: 0 }
    }
}

type Ctx = PluginContext<SideralParams>;

impl truce_egui::EditorUi<SideralParams> for SideralUi {
    fn ui(&mut self, ui: &mut egui::Ui, st: &Ctx) {
        ui.ctx().request_repaint();
        let time = ui.input(|i| i.time) as f32;
        let dt = ui.input(|i| i.stable_dt).min(0.1);
        self.spin = (self.spin + dt * 1.6 * (0.5 + 1.5 * st.get_param(P::Clock))).rem_euclid(TAU);
        let p = ui.painter().clone();
        p.rect_filled(Rect::from_min_size(Pos2::ZERO, vec2(W, H)), 0.0, BG);
        galaxy(&p, time, self.seed);
        starfield(&p, time, self.seed);

        let hint = self.hint.take();
        header(ui, st, hint);
        orrery(ui, st, C, time);
        matter(ui, st, self, time);
        gravity(ui, st, self, time);
        rec_star(ui, st, self, pos2(C.x, ROW_Y), time);
        phenomena(ui, st, self, time);
        horizon(ui, st);
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

fn starfield(p: &egui::Painter, t: f32, seed: u32) {
    for i in 0..190u32 {
        let i = i.wrapping_add(seed);
        let x = hash(i.wrapping_mul(3)) * W;
        let y = hash(i.wrapping_mul(3).wrapping_add(1)) * H;
        let tw = 0.5 + 0.5 * (t * (0.3 + hash(i.wrapping_mul(7)) * 1.2) + hash(i.wrapping_mul(5)) * TAU).sin();
        let a = 0.06 + 0.22 * hash(i.wrapping_mul(3).wrapping_add(2)) * tw;
        p.circle_filled(pos2(x, y), 0.6 + hash(i.wrapping_mul(11)) * 0.8, alpha(Color32::WHITE, a));
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
    let (rl, rr) = (112.0 * K, 82.0 * K);
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
            p.text(c + vec2(0.0, 50.0 * K), Align2::CENTER_CENTER, "awaiting matter · press REC", FontId::proportional(11.0), DIM);
            return;
        }
        1 => {
            let f = len as f32 / MAX_LOOP as f32;
            for (r, col) in [(rl, cl), (rr, cr)] {
                p.circle_stroke(c, r, stroke(1.0, alpha(col, 0.15)));
                p.add(egui::Shape::line(arc(c, r, -PI / 2.0, TAU * f, 96), stroke(2.4, col)));
            }
            p.text(c + vec2(0.0, 50.0 * K), Align2::CENTER_CENTER, "accreting · press REC again to close the orbit", FontId::proportional(11.0), AMBER);
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
                p.line_segment([polar(c, r - 7.0, a), polar(c, r + 7.0, a)], stroke(1.0, alpha(col, 0.5)));
            }
            // The torn-off slice the head is actually playing from.
            let q = sc.read(Slot::SliceQ);
            let a_src = -PI / 2.0 + step * (src as f32 + q);
            p.add(egui::Shape::line(arc(c, r, -PI / 2.0 + step * src as f32, step, 24), stroke(3.4, alpha(col, 0.55))));
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
            p.circle_filled(polar(c, r, a_src), 3.4, col);
        }
        let flash = sc.read(Slot::Flash);
        if (18..=19).contains(&alg) && flash > 0.02 {
            p.circle_stroke(c, (128.0 + (1.0 - flash) * 30.0) * K, stroke(2.0, alpha(ROSE, flash * 0.8)));
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
            p.circle_filled(polar(c, rl - 18.0, a), 3.4, alpha(VIOLET, 0.9));
            p.text(c + vec2(0.0, 50.0 * K), Align2::CENTER_CENTER, if m > 1.0 { "octave up, compounding" } else { "octave down, compounding" }, FontId::proportional(10.5), VIOLET);
        }
    }
    if (13..=14).contains(&alg) {
        p.text(c + vec2(0.0, 50.0 * K), Align2::CENTER_CENTER, if alg == 13 { "8 : 7" } else { "4 : 3" }, FontId::proportional(13.0), DIM);
    }
    planet(rl, a_l, cl, 3.8 + level * 3.0);
    planet(rr, a_r, cr, 3.8 + level * 3.0);
}

/// The galaxy the sun sits in: two arms of dust, turning very slowly.
fn galaxy(p: &egui::Painter, t: f32, seed: u32) {
    let turn = hash(seed) * TAU;
    for n in 0..560u32 {
        let i = n.wrapping_add(seed);
        let u = hash(i.wrapping_mul(5).wrapping_add(1));
        let a = (n % 2) as f32 * PI + turn + u * 5.6 + (hash(i.wrapping_mul(5).wrapping_add(2)) - 0.5) * 0.9 + t * 0.004;
        let r = 70.0 + u * 660.0 + (hash(i.wrapping_mul(5).wrapping_add(3)) - 0.5) * 80.0;
        let q = C + vec2(a.cos() * r, a.sin() * r * 0.66);
        if q.x < 0.0 || q.x > W || q.y < 0.0 || q.y > H {
            continue;
        }
        let col = [CYAN, VIOLET, SAND, TEXT][(n % 4) as usize];
        let h = hash(i.wrapping_mul(5).wrapping_add(4));
        p.circle_filled(q, 0.7 + h * 1.3, alpha(col, 0.05 + 0.11 * h));
    }
}

/// A four-pointed star.
fn star(p: &egui::Painter, c: Pos2, r: f32, col: Color32) {
    let s = r * 2.2;
    p.line_segment([c - vec2(s, 0.0), c + vec2(s, 0.0)], stroke(1.3, alpha(col, 0.8)));
    p.line_segment([c - vec2(0.0, s), c + vec2(0.0, s)], stroke(1.3, alpha(col, 0.8)));
    p.circle_filled(c, r, col);
}

/// A moon in phase. `f` in -1..=1: how much is lit, and from which side
/// (negative from the left).
fn moon(p: &egui::Painter, c: Pos2, r: f32, f: f32, lit: Color32, dark: Color32) {
    p.circle_filled(c, r, dark);
    let k = 1.0 - 2.0 * f.abs().min(1.0);
    let n = (r * 2.0).ceil() as usize;
    let step = 2.0 * r / n as f32;
    for i in 0..n {
        let y = -r + (i as f32 + 0.5) * step;
        let w = (r * r - y * y).max(0.0).sqrt();
        let (x0, x1) = if f >= 0.0 { (k * w, w) } else { (-w, -k * w) };
        if x1 - x0 > 0.4 {
            p.line_segment([c + vec2(x0, y), c + vec2(x1, y)], stroke(step + 0.25, lit));
        }
    }
    p.circle_stroke(c, r, stroke(1.0, alpha(lit, 0.7)));
}

/// Text set along a circle, upright wherever it falls.
fn curved(p: &egui::Painter, c: Pos2, r: f32, mid: f32, text: &str, size: f32, col: Color32) {
    let below = mid.sin() > 0.42;
    let track = 2.4;
    let glyphs: Vec<_> = text.chars().map(|ch| p.layout_no_wrap(ch.to_string(), FontId::proportional(size), col)).collect();
    let total: f32 = glyphs.iter().map(|g| g.size().x + track).sum::<f32>() - track;
    let mut x = -total * 0.5;
    for g in glyphs {
        let size = g.size();
        let off = (x + size.x * 0.5) / r;
        let (a, rot) = if below { (mid - off, mid - off - PI / 2.0) } else { (mid + off, mid + off + PI / 2.0) };
        let at = polar(c, r, a) - size * 0.5;
        p.add(egui::epaint::TextShape::new(at, g, col).with_angle_and_anchor(rot, Align2::CENTER_CENTER));
        x += size.x + track;
    }
}

// ===========================================================================
// Header: the name of the sky, and what it is doing
// ===========================================================================

fn header(ui: &mut egui::Ui, st: &Ctx, hint: Option<(String, Color32)>) {
    let sc = &st.params().scope;
    let (lay, active, quad) = current(st);
    let p = ui.painter();
    let g = pos2(32.0, 27.0);
    p.circle_stroke(g, 11.0, stroke(1.0, alpha(CYAN, 0.7)));
    p.add(egui::Shape::line(
        (0..=40)
            .map(|i| {
                let a = i as f32 / 40.0 * TAU;
                g + vec2(a.cos() * 14.0, a.sin() * 5.0)
            })
            .collect(),
        stroke(1.0, alpha(AMBER, 0.7)),
    ));
    p.circle_filled(g, 2.4, TEXT);
    p.text(pos2(54.0, 27.0), Align2::LEFT_CENTER, "S I D E R A L", FontId::proportional(19.0), TEXT);
    p.text(pos2(W - 16.0, 27.0), Align2::RIGHT_CENTER, "JARDIM SONORO", FontId::proportional(10.0), FAINT);

    // How full the memory is: five faint stars after the name.
    let m = sc.read(Slot::MemoryPeak);
    for (k, &tv) in [0.07, 0.155, 0.31, 0.56, 0.9].iter().enumerate() {
        let col = match k {
            0..=2 => GREEN,
            3 => AMBER,
            _ => RED,
        };
        led(p, pos2(212.0 + k as f32 * 12.0, 27.0), if m >= tv { 1.0 } else { (m / tv).powi(3) * 0.4 }, col);
    }

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
    p.text(pos2(C.x, 10.0), Align2::CENTER_TOP, title, FontId::proportional(15.0), TEXT);
    match hint {
        Some((text, col)) => p.text(pos2(C.x, 31.0), Align2::CENTER_TOP, text, FontId::proportional(10.5), col),
        None => p.text(pos2(C.x, 32.0), Align2::CENTER_TOP, lines.trim_end(), FontId::monospace(9.5), DIM),
    };
}

// ===========================================================================
// The ten bodies: drag one along its orbit
// ===========================================================================

#[derive(Clone, Copy, PartialEq)]
enum Look {
    /// A cloud that spreads.
    Nebula,
    /// A comet; the tail grows.
    Comet,
    /// A star throwing spikes.
    Flare,
    /// A planet whose moon turns at the speed of time.
    Clock,
    /// A planet with an atmosphere that glows with the signal.
    Planet,
    /// A moon lit from the left (ebb) or the right (flow).
    Tide,
    /// A ringed planet: amber decaying, white stable, red escaping.
    Ringed,
    /// A moon that fills with memory.
    Mix,
    /// A double star that draws apart.
    Twin,
}

struct Body<'a> {
    id: P,
    name: &'a str,
    /// -1 in the left sky, +1 in the right.
    side: f32,
    /// 0 is the orbit nearest the sun.
    lane: usize,
    col: Color32,
    look: Look,
    enabled: bool,
    /// The reading under the name.
    value: &'a str,
    /// What it does, shown under the title while it is held.
    hint: &'a str,
    /// A level that makes the body glow, and its colour.
    glow: (f32, Color32),
    /// Where the lit part of the orbit starts.
    origin: f32,
}

fn body(ui: &mut egui::Ui, st: &Ctx, s: &mut SideralUi, b: &Body, t: f32) {
    let raw: u32 = b.id.into();
    let r = LANE0 + LANE_STEP * b.lane as f32;
    let base = if b.side < 0.0 { PI } else { 0.0 };
    let dir = -b.side;
    let ang = |v: f32| base + dir * SPAN * (2.0 * v - 1.0);
    let of_pointer = |q: Pos2| {
        let d = q - C;
        let rel = (d.y.atan2(d.x) - base + PI).rem_euclid(TAU) - PI;
        (dir * rel / SPAN + 1.0) * 0.5
    };

    let mut value = st.get_param(raw);
    let hit = Rect::from_center_size(polar(C, r, ang(value)), Vec2::splat(TOUCH + 6.0));
    let resp = ui.interact(hit, egui::Id::new(("body", raw)), Sense::click_and_drag());
    if resp.double_clicked() {
        if let Some(d) = default_norm(st, raw) {
            set(st, b.id, d);
            value = d as f32;
        }
    } else {
        if resp.drag_started() {
            st.begin_edit(raw);
            if let Some(q) = resp.interact_pointer_pos() {
                s.grab = value - of_pointer(q);
            }
        }
        if resp.dragged()
            && let Some(q) = resp.interact_pointer_pos()
        {
            value = (of_pointer(q) + s.grab).clamp(0.0, 1.0);
            st.set_param(raw, f64::from(value));
        }
        if resp.drag_stopped() {
            st.end_edit(raw);
        }
        if resp.hovered() && !resp.dragged() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll.abs() > 0.0 {
                value = (value + scroll / 1500.0).clamp(0.0, 1.0);
                set(st, b.id, f64::from(value));
            }
        }
    }
    let held = resp.dragged() || resp.is_pointer_button_down_on();
    let active = held || resp.hovered();
    if active {
        s.hint = Some((format!("{}  ·  {}", b.name, b.hint), if b.enabled { b.col } else { DIM }));
    }

    let p = ui.painter();
    let col = if b.enabled { b.col } else { FAINT };
    let a = ang(value);
    let pos = polar(C, r, a);
    // Unit vectors: along the orbit toward more, and away from the sun.
    let up = vec2(-a.sin(), a.cos()) * dir;
    let out = vec2(a.cos(), a.sin());

    // The orbit, lit from its origin to the body.
    p.add(egui::Shape::line(arc(C, r, ang(0.0), ang(1.0) - ang(0.0), 48), stroke(1.0, alpha(FAINT, if active { 0.9 } else { 0.5 }))));
    if (value - b.origin).abs() > 0.004 {
        let a0 = ang(b.origin);
        p.add(egui::Shape::line(arc(C, r, a0, a - a0, 40), stroke(if held { 2.2 } else { 1.6 }, alpha(col, 0.6))));
    }
    for v in [0.0, 1.0] {
        p.circle_filled(polar(C, r, ang(v)), 1.6, alpha(TEXT, 0.45));
    }
    if let Some(d) = default_norm(st, raw) {
        let ad = ang(d as f32);
        p.line_segment([polar(C, r - 4.0, ad), polar(C, r + 4.0, ad)], stroke(1.0, alpha(TEXT, 0.4)));
    }

    // The sphere of influence: where a finger takes hold.
    p.circle_filled(pos, 23.0, alpha(BG, 0.85));
    p.circle_stroke(pos, 23.0, stroke(1.0, alpha(col, if held { 0.9 } else if active { 0.55 } else { 0.2 })));
    let (glow, glow_col) = b.glow;
    let glow = glow.clamp(0.0, 1.0);
    if glow > 0.02 {
        p.circle_filled(pos, 12.0 + 9.0 * glow, alpha(glow_col, 0.10 + 0.22 * glow));
    }

    match b.look {
        Look::Nebula => {
            for k in 0..16u32 {
                let ak = hash(k * 3 + 40) * TAU + t * 0.25 * (hash(k * 3 + 44) - 0.5);
                let d = (2.0 + hash(k * 3 + 41) * 17.0) * (0.3 + 0.7 * value);
                p.circle_filled(pos + vec2(ak.cos(), ak.sin()) * d, 2.5 + hash(k * 3 + 42) * 3.5, alpha(col, 0.10 + 0.12 * hash(k * 3 + 43)));
            }
            p.circle_filled(pos, 3.6, alpha(col, 0.95));
        }
        Look::Comet => {
            let len = 6.0 + 30.0 * value;
            for k in (0..14).rev() {
                let f = k as f32 / 14.0;
                let wob = (t * 2.2 + f * 5.0).sin() * 3.0 * f * value;
                p.circle_filled(pos - up * (len * f) + out * wob, 5.0 * (1.0 - f) + 0.7, alpha(col, 0.45 * (1.0 - f)));
            }
            p.circle_filled(pos, 5.2, lerp_color(col, Color32::WHITE, 0.5));
        }
        Look::Flare => {
            for k in 0..10 {
                let ak = k as f32 / 10.0 * TAU + t * 0.2;
                let l = 8.5 + (1.5 + 12.0 * value) * (0.7 + 0.3 * (t * 3.0 + k as f32 * 1.7).sin());
                p.line_segment([polar(pos, 7.5, ak), polar(pos, l, ak)], stroke(1.4, alpha(col, 0.85)));
            }
            p.circle_filled(pos, 6.5, lerp_color(col, RED, value * 0.55));
        }
        Look::Clock => {
            p.circle_stroke(pos, 16.0, stroke(1.0, alpha(col, 0.35)));
            p.add(egui::Shape::line(arc(pos, 16.0, s.spin, -0.9, 10), stroke(1.6, alpha(col, 0.6))));
            p.circle_filled(polar(pos, 16.0, s.spin), 3.0, TEXT);
            p.circle_filled(pos, 7.5, col);
        }
        Look::Planet => {
            p.circle_filled(pos, 9.5, lerp_color(PANEL, col, 0.3 + 0.7 * value));
            p.circle_stroke(pos, 9.5, stroke(1.2, col));
            p.add(egui::Shape::line(arc(pos, 6.0, PI * 1.15, PI * 0.5, 8), stroke(1.0, alpha(Color32::WHITE, 0.35))));
        }
        Look::Tide => moon(p, pos, 11.0, (value - 0.5) * 2.0, col, PAD),
        Look::Ringed => {
            let ring: Vec<Pos2> = (0..=36).map(|i| i as f32 / 36.0 * TAU).map(|q| pos + vec2(q.cos() * 18.0, q.sin() * 5.5)).collect();
            p.add(egui::Shape::line(ring.clone(), stroke(1.4, alpha(col, 0.75))));
            p.circle_filled(pos, 8.0, col);
            p.add(egui::Shape::line(ring[..=18].to_vec(), stroke(1.4, alpha(col, 0.75))));
        }
        Look::Mix => moon(p, pos, 11.0, value, col, PAD),
        Look::Twin => {
            let d = vec2(3.5 + 11.0 * value, 0.0);
            p.line_segment([pos - d, pos + d], stroke(1.0, alpha(col, 0.35)));
            p.circle_filled(pos - d, 4.2, col);
            p.circle_filled(pos + d, 3.4, lerp_color(col, Color32::WHITE, 0.45));
        }
    }

    // Name and reading ride with the body, on alternate sides of the lanes.
    let lc = pos + up * if b.lane % 2 == 1 { 37.0 } else { -37.0 };
    p.rect_filled(Rect::from_center_size(lc, vec2(b.name.len().max(b.value.len()) as f32 * 6.2 + 8.0, 26.0)), 6.0, alpha(BG, 0.8));
    p.text(lc - vec2(0.0, 6.0), Align2::CENTER_CENTER, b.name, FontId::proportional(10.5), if !b.enabled { FAINT } else if active { TEXT } else { lerp_color(DIM, TEXT, 0.6) });
    p.text(lc + vec2(0.0, 6.5), Align2::CENTER_CENTER, b.value, FontId::proportional(9.0), if active { col } else { DIM });
}

fn percent(st: &Ctx, id: P) -> String {
    format!("{:.0}%", st.get_param(id) * 100.0)
}

/// The left sky: what the memory is made of.
fn matter(ui: &mut egui::Ui, st: &Ctx, s: &mut SideralUi, t: f32) {
    let (lay, alg, _) = current(st);
    let sc = &st.params().scope;
    let nova = (18..=19).contains(&alg);
    let kepler = alg == 20;
    let rest = lay.family == Family::Reverb;
    let scatter = match lay.family {
        Family::TwoDelays | Family::FourDelays => "lines bleed into each other",
        Family::Reverb => "at rest in the nebula",
        Family::Granular => "grains scattered by noise",
        Family::Pulsar => "the Halo: a small plate reverb",
    };
    let drift = match lay.family {
        Family::TwoDelays | Family::FourDelays => "lines wander across the sky, chaotically",
        Family::Reverb => "at rest in the nebula",
        Family::Granular => "grains wander on chaotic tides",
        Family::Pulsar if kepler => "down · still · up: an octave, recursively",
        Family::Pulsar if nova => "past noon: random octave falls",
        Family::Pulsar => "the loops wander across the sky",
    };
    let pct = |id: P| if rest { "at rest".to_owned() } else { percent(st, id) };
    let none = (0.0, CYAN);
    let lvl = sc.read(Slot::InputPeak);
    let level = |v: f32| ((v / 0.3).min(1.0), if v > 0.95 { RED } else { GREEN });

    let v = pct(P::Blur);
    body(ui, st, s, &Body { id: P::Blur, name: "SCATTER", side: -1.0, lane: 0, col: CYAN, look: Look::Nebula, enabled: !rest, value: &v, hint: scatter, glow: none, origin: 0.0 }, t);
    let v = pct(P::Drift);
    body(ui, st, s, &Body { id: P::Drift, name: "DRIFT", side: -1.0, lane: 1, col: CYAN, look: Look::Comet, enabled: !rest, value: &v, hint: drift, glow: none, origin: 0.0 }, t);
    let v = percent(st, P::Drive);
    body(ui, st, s, &Body { id: P::Drive, name: "FLARE", side: -1.0, lane: 2, col: AMBER, look: Look::Flare, enabled: true, value: &v, hint: "odd harmonics, burning in the loop", glow: none, origin: 0.0 }, t);
    let v = st.format_param(P::Clock);
    body(ui, st, s, &Body { id: P::Clock, name: "DILATION", side: -1.0, lane: 3, col: VIOLET, look: Look::Clock, enabled: true, value: &v, hint: "the speed of time itself", glow: none, origin: 1.0 / 3.0 }, t);
    let v = st.format_param(P::Input);
    body(ui, st, s, &Body { id: P::Input, name: "IN", side: -1.0, lane: 4, col: GREEN, look: Look::Planet, enabled: true, value: &v, hint: "how much reaches the sun", glow: level(lvl), origin: 0.5 }, t);
}

/// The right sky: what holds the memory together.
fn gravity(ui: &mut egui::Ui, st: &Ctx, s: &mut SideralUi, t: f32) {
    let sc = &st.params().scope;
    let none = (0.0, CYAN);

    let fbv = crate::engine::fb_gain(st.get_param(P::Feedback));
    let (state, col) = if fbv > 1.0005 {
        ("escaping: it grows", RED)
    } else if fbv > 0.995 {
        ("a stable orbit", TEXT)
    } else {
        ("a decaying orbit", AMBER)
    };
    let v = format!("×{fbv:.3}");
    body(ui, st, s, &Body { id: P::Feedback, name: "ORBIT", side: 1.0, lane: 0, col, look: Look::Ringed, enabled: true, value: &v, hint: state, glow: (sc.read(Slot::Limiter) * 4.0, RED), origin: 0.0 }, t);

    let tide = st.get_param(P::Supcom);
    let hint = if tide < 0.49 {
        "ebb: each new sound washes the old away"
    } else if tide > 0.51 {
        "flow: soft rises, loud settles"
    } else {
        "ebb below · still · flow above"
    };
    let v = st.format_param(P::Supcom);
    body(ui, st, s, &Body { id: P::Supcom, name: "TIDE", side: 1.0, lane: 1, col: VIOLET, look: Look::Tide, enabled: true, value: &v, hint, glow: (sc.read(Slot::Suppress), VIOLET), origin: 0.5 }, t);

    let v = percent(st, P::Mix);
    body(ui, st, s, &Body { id: P::Mix, name: "MIX", side: 1.0, lane: 2, col: CYAN, look: Look::Mix, enabled: true, value: &v, hint: "memory against the present", glow: none, origin: 0.0 }, t);
    let v = st.format_param(P::Width);
    body(ui, st, s, &Body { id: P::Width, name: "PARALLAX", side: 1.0, lane: 3, col: CYAN, look: Look::Twin, enabled: true, value: &v, hint: "from a single point to wider than the speakers", glow: none, origin: 0.5 }, t);
    let out = sc.read(Slot::OutputPeak);
    let v = st.format_param(P::Output);
    body(ui, st, s, &Body { id: P::Output, name: "OUT", side: 1.0, lane: 4, col: GREEN, look: Look::Planet, enabled: true, value: &v, hint: "how much leaves the sky", glow: ((out / 0.3).min(1.0), if out > 0.95 { RED } else { GREEN }), origin: 2.0 / 3.0 }, t);
}

// ===========================================================================
// The orrery and the dial of programs
// ===========================================================================

fn dial_angle(i: usize) -> f32 {
    let start = PI * (2.0 / 3.0);
    let span = PI * (5.0 / 3.0);
    let gap = 0.12;
    let step = (span - 4.0 * gap) / 20.0;
    let fam = FAMILIES.iter().position(|f| i >= f.0 && i < f.0 + f.1).unwrap_or(0);
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
    let r_mem = R_MEM;
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
        let len = 2.0 + v * 24.0 * K;
        p.line_segment([polar(c, r_mem + 1.0, a), polar(c, r_mem + 1.0 + len, a)], stroke(1.9, alpha(col, 0.22 + 0.7 * v)));
    }
    if lay.family != Family::Pulsar {
        let mut b = 0;
        for &n in lay.lines.iter() {
            let a = -PI / 2.0 + TAU * b as f32 / MEM as f32;
            p.line_segment([polar(c, r_mem - 6.0, a), polar(c, r_mem + 6.0, a)], stroke(1.0, alpha(TEXT, 0.55)));
            b += n;
        }
    }
    if dust {
        for i in 0..90u32 {
            let k = i + (t * 24.0) as u32 * 97;
            let a = hash(k) * TAU;
            let rr = (30.0 + hash(k + 1) * 160.0) * K;
            p.circle_filled(polar(c, rr, a), 1.1, alpha(SAND, 0.25 + 0.5 * hash(k + 2)));
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
            p.line_segment([polar(c, r, a0), polar(c, r, a1)], stroke(2.3, alpha(col, 0.5 * (1.0 - k as f32 / 14.0))));
        }
        let size = size * K;
        p.circle_filled(polar(c, r, a), size + 4.5, alpha(col, 0.15 + 0.2 * level));
        p.circle_filled(polar(c, r, a), size, col);
    };

    if lay.family == Family::Pulsar {
        pulsar_orbits(&p, st, c, active, t, &shade, &planet);
    } else {
        let max_len = 1_097_101.0f32;
        let radius = |n: usize| (32.0 + 100.0 * (n as f32 / max_len).sqrt()) * K;
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
                    0 => (r - 10.0 * K, CYAN),
                    1 => (r + 10.0 * K, AMBER),
                    _ => (r, VIOLET),
                };
                let a = -PI / 2.0 + TAU * g;
                p.circle_filled(polar(c, rr, a), 2.2 * K, alpha(shade(col), 0.9));
            }
        }
    }

    // --- the sun, the LFOs, the eclipse ---------------------------------------
    let lvl = sc.read(Slot::InputPeak).min(1.0);
    p.circle_filled(c, (16.0 + lvl * 10.0) * K, alpha(AMBER, 0.06 + 0.12 * lvl));
    p.circle_filled(c, (7.0 + lvl * 3.0) * K, lerp_color(Color32::from_rgb(120, 90, 40), AMBER, 0.4 + lvl));
    if eclipse {
        let pulse = 0.5 + 0.5 * (t * 1.3).sin();
        for k in 0..3 {
            p.circle_stroke(c, (13.0 + k as f32 * 5.0 + pulse * 2.0) * K, stroke(1.0, alpha(Color32::WHITE, 0.25 - k as f32 * 0.07)));
        }
        p.circle_filled(c + vec2(1.5, -1.0), 11.0 * K, Color32::from_rgb(4, 4, 8));
    }
    if matches!(lay.family, Family::TwoDelays | Family::FourDelays | Family::Granular | Family::Pulsar) {
        let n = if matches!(lay.family, Family::FourDelays | Family::Granular) { 4 } else { 2 };
        for k in 0..n {
            let v = sc.get(Slot::Lfo0 as usize + k).clamp(0.0, 1.0);
            let a = -PI / 2.0 + k as f32 * TAU / n as f32 + (v - 0.5) * 1.2;
            p.circle_filled(polar(c, 22.0 * K, a), 1.8, alpha(TEXT, 0.7));
        }
    }

    // --- the dial: twenty-one stars, their names written along it ----------------
    let r_dial = R_DIAL;
    let settle = sc.read(Slot::Settle);
    let switching = sc.read(Slot::Switching) > 0.5;
    for &(name, first, count) in GROUPS.iter() {
        let a0 = dial_angle(first) - 0.06;
        let a1 = dial_angle(first + count - 1) + 0.06;
        let on = selected >= first && selected < first + count;
        p.add(egui::Shape::line(arc(c, r_dial + 14.0, a0, a1 - a0, 24), stroke(1.0, alpha(if on { TEXT } else { FAINT }, 0.8))));
        curved(&p, c, r_dial + 26.0, (a0 + a1) * 0.5, name, 9.5, if on { TEXT } else { DIM });
    }
    for i in 0..21 {
        let a = dial_angle(i);
        let pos = polar(c, r_dial, a);
        let hit = Rect::from_center_size(pos, Vec2::splat(TOUCH));
        let resp = ui.interact(hit, egui::Id::new(("prog", i)), Sense::click());
        if resp.clicked() {
            set_enum(st, P::Program, i);
        }
        // The target is finger-sized, but the name only shows when the
        // pointer is on the star itself.
        let size = if i < 12 { [4.0, 5.8, 7.6][i % 3] } else { 6.0 };
        let near = ui.input(|inp| inp.pointer.latest_pos()).is_some_and(|q| (q - pos).length() <= size + 4.0);
        let col = if i == active {
            TEXT
        } else if i == selected {
            AMBER
        } else if near {
            DIM
        } else {
            lerp_color(FAINT, DIM, 0.4)
        };
        if i == active {
            p.circle_filled(pos, 15.0, alpha(TEXT, 0.08));
        }
        if i < 12 {
            p.circle_filled(pos, size, col);
        } else {
            star(&p, pos, 2.6, col);
        }
        if i == selected {
            p.circle_stroke(pos, 13.0, stroke(1.0, alpha(AMBER, 0.55)));
            if switching {
                p.add(egui::Shape::line(arc(pos, 13.0, -PI / 2.0, TAU * settle, 24), stroke(2.0, AMBER)));
            }
        }
        if near {
            p.text(polar(c, r_dial - 24.0, a), Align2::CENTER_CENTER, NAMES[i], FontId::proportional(10.5), TEXT);
        }
    }

    // Drag along the dial to sweep through the programs.
    let ring = Rect::from_center_size(c, Vec2::splat(r_dial * 2.0 + 30.0));
    let resp = ui.interact(ring, egui::Id::new("dial-drag"), Sense::drag());
    if resp.dragged()
        && let Some(pp) = resp.interact_pointer_pos()
    {
        let d = pp - c;
        if d.length() > r_dial - 28.0 && d.length() < r_dial + 34.0 {
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

// ===========================================================================
// REC: a red giant in the middle of the row, with its two moons
// ===========================================================================

/// Tap the giant to start or stop feeding the memory. Its moons choose which
/// side of the sky receives new matter; both lit is L+R.
fn rec_star(ui: &mut egui::Ui, st: &Ctx, s: &mut SideralUi, c: Pos2, t: f32) {
    let rec = enum_index(st, P::Record);
    if rec != 0 {
        s.last_rec = rec;
    }
    let on = rec != 0;
    let r = 32.0;
    let resp = ui.interact(Rect::from_center_size(c, Vec2::splat(r * 2.0)), egui::Id::new("rec"), Sense::click());
    if resp.clicked() {
        set_enum(st, P::Record, if on { 0 } else { s.last_rec });
    }
    let p = ui.painter();
    let breath = 0.5 + 0.5 * (t * 3.2).sin();
    let down = resp.is_pointer_button_down_on();
    for k in 0..4 {
        let f = k as f32 / 4.0;
        p.circle_filled(c, r + 12.0 * (1.0 - f) + if on { breath * 3.0 } else { 0.0 }, alpha(RED, if on { 0.07 } else { 0.025 }));
    }
    p.circle_filled(c, r, if on { lerp_color(PAD, RED, 0.34) } else { lerp_color(PAD, RED, 0.08) });
    p.circle_stroke(c, r, stroke(if on { 1.8 } else { 1.0 }, alpha(RED, if on || resp.hovered() { 0.9 } else { 0.4 })));
    // Matter falling in while it listens.
    for k in 0..24 {
        let a = k as f32 / 24.0 * TAU + if on { t * 0.9 } else { 0.0 };
        let lit = if on { 0.25 + 0.6 * ((k as f32 / 24.0 + t * 0.4).fract()) } else { 0.18 };
        p.add(egui::Shape::line(arc(c, r - 6.0, a, 0.12, 2), stroke(1.2, alpha(if on { AMBER } else { DIM }, lit))));
    }
    if down {
        p.circle_filled(c, r, alpha(Color32::WHITE, 0.07));
    }
    p.circle_filled(c - vec2(0.0, 6.0), 7.5 + if on { breath * 1.5 } else { 0.0 }, if on { RED } else { lerp_color(PAD, RED, 0.5) });
    p.text(c + vec2(0.0, 13.0), Align2::CENTER_CENTER, "REC", FontId::proportional(11.5), if on { TEXT } else { lerp_color(DIM, TEXT, 0.5) });

    for (bit, label, dx) in [(1usize, "L", -1.0f32), (2, "R", 1.0)] {
        let mc = c + vec2(dx * (r + 27.0), 2.0);
        let mr = 14.0;
        let mresp = ui.interact(Rect::from_center_size(mc, Vec2::splat(TOUCH)), egui::Id::new(("recmoon", bit)), Sense::click());
        if mresp.clicked() {
            let mask = s.last_rec ^ bit;
            if mask != 0 {
                s.last_rec = mask;
                if on {
                    set_enum(st, P::Record, mask);
                }
            }
        }
        let sel = s.last_rec & bit != 0;
        let p = ui.painter();
        let fill = match (sel, on) {
            (true, true) => alpha(AMBER, 0.9),
            (true, false) => alpha(AMBER, 0.3),
            _ => Color32::from_rgb(26, 29, 42),
        };
        p.circle_filled(mc, mr, fill);
        p.circle_stroke(mc, mr, stroke(1.0, alpha(AMBER, if mresp.hovered() || mresp.is_pointer_button_down_on() { 0.9 } else { 0.3 })));
        p.text(mc, Align2::CENTER_CENTER, label, FontId::proportional(10.5), if sel && on { BG } else if sel { TEXT } else { DIM });
    }
}

// ===========================================================================
// Phenomena: six things that happen to the sky. Tap, or hold.
// ===========================================================================

#[derive(Clone, Copy)]
enum Kind {
    Retrograde,
    Eclipse,
    Erode,
    Redshift,
    Dust,
    Crunch,
}

/// One phenomenon, its name under it. `on` is 0..1; `squeeze` is BIG
/// CRUNCH's collapse.
#[allow(clippy::too_many_arguments)]
fn phenomenon(ui: &mut egui::Ui, kind: Kind, slot: f32, on: f32, col: Color32, title: &str, sub: &str, enabled: bool, squeeze: f32, t: f32) -> egui::Response {
    let c = pos2(C.x + slot * ROW_STEP, ROW_Y - 4.0);
    let hit = Rect::from_min_max(c - vec2(32.0, 30.0), c + vec2(32.0, 52.0));
    let resp = ui.interact(hit, egui::Id::new(("phen", title)), Sense::click_and_drag());
    let glow = ui.ctx().animate_value_with_time(egui::Id::new(("phen-glow", title)), on.clamp(0.0, 1.0), 0.22);
    let lit = glow > 0.5;
    let p = ui.painter();
    let base = if enabled { col } else { FAINT };
    let ink = if !enabled { FAINT } else { lerp_color(lerp_color(DIM, col, 0.35), col, glow) };
    let down = resp.is_pointer_button_down_on();

    p.circle_filled(c, 27.0, alpha(BG, 0.85));
    if glow > 0.01 {
        p.circle_filled(c, 34.0, alpha(base, 0.06 * glow));
        p.circle_filled(c, 27.0, alpha(base, 0.10 * glow));
    }
    p.circle_stroke(c, 27.0, stroke(1.0, alpha(base, if down { 0.95 } else if resp.hovered() { 0.6 } else { 0.18 + 0.5 * glow })));

    let s = stroke(1.7, ink);
    match kind {
        Kind::Retrograde => {
            // A moon that turns the other way.
            let a = if lit { -t * 1.5 } else { t * 0.6 };
            let tail = if lit { 1.3 } else { -1.3 };
            p.circle_stroke(c, 17.0, stroke(1.0, alpha(ink, 0.35)));
            p.add(egui::Shape::line(arc(c, 17.0, a, tail, 12), stroke(2.0, alpha(ink, 0.7))));
            p.circle_filled(polar(c, 17.0, a), 3.6, ink);
            p.circle_filled(c, 6.5, ink);
        }
        Kind::Eclipse => {
            // The moon slides over the sun.
            let pulse = 0.5 + 0.5 * (t * 1.3).sin();
            for k in 0..3 {
                p.circle_stroke(c, 14.0 + k as f32 * 3.5 + pulse * 1.5, stroke(1.0, alpha(ink, glow * (0.4 - k as f32 * 0.11))));
            }
            p.circle_filled(c, 12.5, ink);
            p.circle_filled(c + vec2(15.0, -6.0) * (1.0 - glow), 11.5 + glow, BG);
        }
        Kind::Erode => {
            // An asteroid, shedding.
            let rock: Vec<Pos2> = (0..11u32).map(|k| polar(c, 10.0 + hash(k + 70) * 7.0, k as f32 / 11.0 * TAU)).collect();
            p.add(egui::Shape::closed_line(rock, s));
            p.circle_filled(c + vec2(-3.0, -2.0), 2.4, alpha(ink, 0.5));
            p.circle_filled(c + vec2(4.0, 4.0), 1.6, alpha(ink, 0.5));
            if lit {
                for k in 0..10u32 {
                    let f = (t * 1.4 + hash(k + 90)).fract();
                    p.circle_filled(polar(c, 14.0 + f * 16.0, hash(k + 80) * TAU), 1.8 * (1.0 - f) + 0.4, alpha(ink, 1.0 - f));
                }
            }
        }
        Kind::Redshift => {
            // Waves stretched behind a receding star.
            let star_at = c + vec2(7.0 * glow, 0.0);
            for k in 0..3 {
                let f = if lit { (k as f32 / 3.0 + t * 0.45).fract() } else { (k as f32 + 1.0) / 3.5 };
                let rr = 5.0 + f * 16.0;
                p.circle_stroke(star_at - vec2(f * 13.0 * glow, 0.0), rr, stroke(1.4, alpha(ink, if lit { 1.0 - f } else { 0.6 })));
            }
            p.circle_filled(star_at, 4.5, ink);
        }
        Kind::Dust => {
            for k in 0..24u32 {
                let k2 = k + if lit { (t * 8.0) as u32 * 31 } else { 0 };
                let q = polar(c, hash(k2 * 2 + 3).sqrt() * 19.0, hash(k2 * 2 + 4) * TAU);
                p.circle_filled(q, 1.0 + hash(k * 7 + 5) * 1.1, alpha(ink, if lit { 0.5 + 0.5 * hash(k2) } else { 0.75 }));
            }
        }
        Kind::Crunch => {
            // A black hole and its disc, which falls in while you hold.
            let k = 1.0 - 0.75 * squeeze.clamp(0.0, 1.0);
            let disc: Vec<Pos2> = (0..=40).map(|i| i as f32 / 40.0 * TAU).map(|q| c + vec2(q.cos() * 20.0 * k, q.sin() * 6.5 * k)).collect();
            p.add(egui::Shape::line(disc.clone(), s));
            p.circle_filled(c, 9.0, Color32::BLACK);
            p.circle_stroke(c, 9.0, stroke(1.3, ink));
            p.add(egui::Shape::line(disc[..=20].to_vec(), s));
        }
    }

    p.text(c + vec2(0.0, 36.0), Align2::CENTER_CENTER, title, FontId::proportional(10.5), if !enabled { FAINT } else if lit { TEXT } else { lerp_color(DIM, TEXT, 0.55) });
    p.text(c + vec2(0.0, 48.0), Align2::CENTER_CENTER, sub, FontId::proportional(8.0), if enabled { DIM } else { FAINT });
    resp
}

fn phenomena(ui: &mut egui::Ui, st: &Ctx, s: &mut SideralUi, t: f32) {
    let (lay, _, _) = current(st);
    let sc = &st.params().scope;
    let b = |v: bool| if v { 1.0 } else { 0.0 };
    // Steps along the row either side of REC; negative is to the left.
    let slots = [-3.4, -2.4, -1.4, 1.4, 2.4, 3.4];

    let retro = is_on(st, P::Reverse);
    if phenomenon(ui, Kind::Retrograde, slots[0], b(retro), CYAN, "RETROGRADE", "the tape turns back", true, 0.0, t).clicked() {
        set(st, P::Reverse, if retro { 0.0 } else { 1.0 });
    }

    let ecl = is_on(st, P::Eclipse);
    if phenomenon(ui, Kind::Eclipse, slots[1], b(ecl), Color32::from_rgb(230, 230, 240), "ECLIPSE", "memory held, untouched", true, 0.0, t).clicked() {
        set(st, P::Eclipse, if ecl { 0.0 } else { 1.0 });
    }

    // ERODE: hold to wear away what is playing.
    let erode = is_on(st, P::Erase);
    let resp = phenomenon(ui, Kind::Erode, slots[2], b(erode), ROSE, "ERODE", "hold to wear away", true, 0.0, t);
    let held = resp.is_pointer_button_down_on();
    if held != erode {
        set(st, P::Erase, if held { 1.0 } else { 0.0 });
    }

    let rs = is_on(st, P::HalfSpeed);
    let rs_ok = matches!(lay.family, Family::TwoDelays | Family::FourDelays | Family::Pulsar);
    let rs_sub = if rs_ok { "half speed, an octave down" } else { "binaries, trapezia, pulsars" };
    if phenomenon(ui, Kind::Redshift, slots[3], b(rs), RED, "REDSHIFT", rs_sub, rs_ok, 0.0, t).clicked() {
        set(st, P::HalfSpeed, if rs { 0.0 } else { 1.0 });
    }

    let dust = is_on(st, P::Grit);
    if phenomenon(ui, Kind::Dust, slots[4], b(dust), SAND, "DUST", "sample & hold grain", true, 0.0, t).clicked() {
        set(st, P::Grit, if dust { 0.0 } else { 1.0 });
    }

    // BIG CRUNCH: hold for 0.8 s to collapse the whole memory.
    let now = ui.input(|i| i.time);
    let clearing = sc.read(Slot::Clearing) > 0.5;
    let progress = s.crunch_start.map_or(0.0, |t0| ((now - t0) / 0.8) as f32);
    let held = if progress.is_finite() { progress.min(1.0) } else { 1.0 };
    let glow = if clearing { 1.0 } else if progress.is_finite() { progress * 0.8 } else { 0.0 };
    let resp = phenomenon(ui, Kind::Crunch, slots[5], glow, VIOLET, "BIG CRUNCH", "hold to collapse it all", true, held, t);
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
        let c = pos2(C.x + slots[5] * ROW_STEP, ROW_Y - 4.0);
        ui.painter().add(egui::Shape::line(arc(c, 27.0, -PI / 2.0, TAU * progress.min(1.0), 40), stroke(2.4, VIOLET)));
    }
}

// ===========================================================================
// The horizon: six constellations, one star lit in each
// ===========================================================================

/// Distance between the stars of a constellation.
const PITCH: f32 = 48.0;

/// Draws one constellation starting at `x`; returns the star that was tapped.
#[allow(clippy::too_many_arguments)]
fn constellation(ui: &mut egui::Ui, key: &str, caption: &str, labels: &[&str], lit: usize, x: f32, accent: Color32, enabled: bool) -> Option<usize> {
    let y = 743.0;
    let accent = if enabled { accent } else { FAINT };
    let at = |i: usize| pos2(x + PITCH * (i as f32 + 0.5), y + (hash(i as u32 * 13 + key.len() as u32 * 7) - 0.5) * 13.0);
    let mut tapped = None;
    ui.painter().text(pos2(x + 6.0, 713.0), Align2::LEFT_CENTER, caption, FontId::proportional(9.0), if enabled { lerp_color(DIM, TEXT, 0.5) } else { FAINT });
    for i in 1..labels.len() {
        let (a, b) = (at(i - 1), at(i));
        let d = (b - a).normalized();
        ui.painter().line_segment([a + d * 10.0, b - d * 10.0], stroke(1.0, alpha(FAINT, 0.9)));
    }
    for (i, label) in labels.iter().enumerate() {
        let c = at(i);
        let hit = Rect::from_min_max(pos2(c.x - PITCH * 0.5, y - 22.0), pos2(c.x + PITCH * 0.5, y + 36.0));
        let resp = ui.interact(hit, egui::Id::new(("const", key, i)), Sense::click());
        if resp.clicked() {
            tapped = Some(i);
        }
        let p = ui.painter();
        let on = i == lit;
        let touched = resp.hovered() || resp.is_pointer_button_down_on();
        if on {
            p.circle_filled(c, 13.0, alpha(accent, 0.12));
            star(p, c, 4.6, accent);
        } else {
            p.circle_filled(c, 3.4, if touched { DIM } else { lerp_color(FAINT, DIM, 0.35) });
        }
        if touched {
            p.circle_stroke(c, 13.0, stroke(1.0, alpha(accent, 0.6)));
        }
        p.text(pos2(c.x, y + 25.0), Align2::CENTER_CENTER, *label, FontId::proportional(9.0), if on && enabled { TEXT } else if enabled { DIM } else { FAINT });
    }
    tapped
}

fn horizon(ui: &mut egui::Ui, st: &Ctx) {
    let (lay, _, _) = current(st);
    let pulsar = lay.family == Family::Pulsar;
    let gap = (W - 28.0 - PITCH * 20.0) / 5.0;
    let mut x = 14.0;
    let mut next = |n: usize| {
        let at = x;
        x += PITCH * n as f32 + gap;
        at
    };
    let mut pick = |ui: &mut egui::Ui, id: P, caption: &str, labels: &[&str], accent: Color32, enabled: bool| {
        let at = next(labels.len());
        if let Some(i) = constellation(ui, caption, caption, labels, enum_index(st, id), at, accent, enabled) {
            set_enum(st, id, i);
        }
    };
    pick(ui, P::Constellation, "PRIMES", &["I", "II"], CYAN, true);
    pick(ui, P::Hpf, "LOW CUT · Hz", &["OFF", "266", "462", "786"], GREEN, true);
    pick(ui, P::Lpf, "HIGH CUT · Hz", &["OFF", "622", "1.2k", "5.2k"], GREEN, true);
    pick(ui, P::Period, if pulsar { "PERIOD · host bars" } else { "PERIOD · pulsars only" }, &["FREE", "1", "2", "4", "8"], AMBER, pulsar);
    pick(ui, P::Gate, "HORIZON · dB", &["OPEN", "−48", "−68"], VIOLET, true);
    pick(ui, P::Mono, "INPUT", &["STEREO", "SUMMED"], CYAN, true);
}
