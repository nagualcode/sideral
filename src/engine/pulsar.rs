//! The PULSAR family: the nine programs of the rhythmic firmware
//! (`COSMOS_R1.51`), transcribed from its routines `FUN_08022960` (looper)
//! … `FUN_0802b1d4` (fractal pitch shifter).
//!
//! The rhythmic firmware splits the sound memory into two 2^20-float halves,
//! one per channel, and keeps a loop of `len` samples at the start of each
//! (at most 898 000 samples, 19.2 s at 46 875 Hz). The loop length survives
//! every change of program inside the family, as does the playing position.
//!
//! * Pulsar        — a plain looper (prog. 1).
//! * Resonance 8:7 — the right loop is 7/8 of the left (prog. 2).
//! * Resonance 4:3 — the right loop is 3/4 of the left (prog. 3).
//! * Roche ÷8/÷6/÷5 — the loop is cut in slices; every slice plays (and is
//!   rewritten into) a randomly drawn slice, so the memory itself is
//!   shuffled in place (prog. 4–6).
//! * Supernova ÷8/÷6 — slices with stutter subdivisions, an accelerating
//!   ratchet, reversal, octave jumps, drop-outs and swells (prog. 7–8).
//! * Kepler — the loop is read at twice or half its rate and written back
//!   where it was read from, so the transposition compounds every pass:
//!   a fractal pitch shifter (prog. 9).
//!
//! In the whole family SCATTER (the hardware's BLUR) adds the firmware's
//! small plate reverb, "Halo", built on a 16 384-float SRAM ring.

use super::layouts::MEM;
use super::{Engine, Knobs, drive_shape};

/// Base of the right-channel half.
pub const RB: usize = 1_048_575;
/// Longest loop the firmware accepts (19.16 s at 46 875 Hz).
pub const MAX_LOOP: usize = 898_000;
/// Slice edge fades.
const EDGE: usize = 200;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum LoopState {
    #[default]
    Empty,
    Defining,
    Defined,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Loop,
    Resonance(usize, usize),
    Roche(usize),
    Supernova(usize),
    Kepler,
}

pub fn kind(program: usize) -> Kind {
    match program {
        12 => Kind::Loop,
        13 => Kind::Resonance(7, 8),
        14 => Kind::Resonance(3, 4),
        15 => Kind::Roche(8),
        16 => Kind::Roche(6),
        17 => Kind::Roche(5),
        18 => Kind::Supernova(8),
        19 => Kind::Supernova(6),
        _ => Kind::Kepler,
    }
}

/// The Halo plate (from the rhythmic firmware): a figure-of-eight of four
/// all-passes and four delays packed into one ring by tap offsets, two of the
/// all-passes swept by slow triangles.
pub struct Halo {
    buf: Vec<f32>,
    u: usize,
    m1: f32,
    m2: f32,
    i1: f32,
    i2: f32,
    carry: f32,
}

const HMASK: usize = 0x3fff;

impl Halo {
    pub fn new() -> Self {
        Self { buf: vec![0.0; HMASK + 1], u: 0, m1: 0.0, m2: 0.0, i1: 0.002_103, i2: 0.001_214, carry: 0.0 }
    }

    pub fn clear(&mut self) {
        self.buf.fill(0.0);
        self.carry = 0.0;
    }

    #[inline]
    fn at(&self, k: usize) -> f32 {
        self.buf[(self.u + k) & HMASK]
    }
    #[inline]
    fn put(&mut self, k: usize, v: f32) {
        self.buf[(self.u + k) & HMASK] = v;
    }
    #[inline]
    fn lerp(&self, k: usize, m: f32) -> f32 {
        let mi = m as usize;
        let f = m - mi as f32;
        (1.0 - f) * self.at(k + mi) + self.at(k + mi + 1) * f
    }

    /// Process one tick; returns the wet stereo pair. `blur` is the knob.
    #[inline]
    pub fn tick(&mut self, l: f32, r: f32, blur: f32) -> (f32, f32) {
        let x = ((l + r) * 0.125).clamp(-1.0, 1.0);
        let d = blur * 0.4 + 0.5;
        let g = (1.1 - d * d * d) * 0.55;

        let w0 = self.carry + x;
        let r1 = self.lerp(0, self.m1);
        self.put(2011, w0 + r1 * g);
        let a = self.at(2012);
        self.put(3205, r1 - w0 * g);
        let v = a * d + x;
        let b = self.at(3206);
        self.put(4989, v + b * g);
        let c = self.at(4990);
        self.put(7293, b - v * g);
        let v2 = c * d + x;
        let r3 = self.lerp(7294, self.m2);
        self.put(8157, v2 + r3 * g);
        let e = self.at(8158);
        self.put(10_441, r3 - v2 * g);
        let v3 = e * d + x;
        let f = self.at(10_442);
        self.put(13_391, v3 + f * g);
        self.carry = -(self.at(13_392) * d);
        self.put(14_765, f - v3 * g);

        let ol = self.at(2213) * 0.8 + self.at(5336) * 0.5 + self.at(9201) * 0.58 + self.at(15_015) * 0.4 * d;
        let or = self.at(2709) * 0.6 + self.at(6532) * 0.7 + self.at(10_187) * 0.4 + self.at(16_201) * 0.5 * d;
        self.u = (self.u + 1) & HMASK;

        self.m1 += self.i1;
        if self.m1 >= 53.0 {
            self.i1 = -0.002_103;
        }
        if self.m1 < 0.0 {
            self.i1 = 0.002_103;
            self.m1 = 0.0;
        }
        self.m2 += self.i2;
        if self.m2 >= 71.0 {
            self.i2 = -0.001_214;
        }
        if self.m2 < 0.0 {
            self.i2 = 0.001_214;
            self.m2 = 0.0;
        }
        (ol, or)
    }
}

impl Default for Halo {
    fn default() -> Self {
        Self::new()
    }
}

/// Everything the family keeps between ticks.
#[derive(Default, Clone, Copy, Debug)]
pub struct PulsarState {
    pub state: LoopState,
    pub len: usize,
    pub pos: usize,
    pub pos_r: usize,
    /// Slice machinery (Roche / Supernova).
    pub q: usize,
    pub src_l: usize,
    pub src_r: usize,
    pub rep: usize,
    pub ratchet: bool,
    pub reverse: bool,
    pub up: bool,
    pub down: bool,
    pub ev_l: u32,
    pub ev_r: u32,
    pub rng: u32,
    /// Kepler smoothing.
    pub sm: [f32; 2],
    /// Last event flash for the editor (0..1).
    pub flash: f32,
}

#[inline]
fn edge(i: usize, n: usize) -> f32 {
    if i < EDGE {
        i as f32 / EDGE as f32
    } else if i + EDGE > n {
        n.saturating_sub(i) as f32 / EDGE as f32
    } else {
        1.0
    }
}

#[inline]
fn xorshift(x: &mut u32) -> u32 {
    let mut v = *x;
    v ^= v << 13;
    v ^= v >> 17;
    v ^= v << 5;
    *x = v;
    v
}

impl Engine {
    /// Loop length bookkeeping: free (defined by RECORD) or locked to the
    /// host's bars.
    fn pulsar_transport(&mut self, rec_on: bool, sync_len: Option<usize>) {
        let p = &mut self.pulsar;
        match (p.state, sync_len) {
            (LoopState::Empty, Some(n)) if rec_on => {
                p.state = LoopState::Defined;
                p.len = n.clamp(400, MAX_LOOP);
                p.pos = self.sync_phase.map_or(0, |ph| ((ph * p.len as f64) as usize).min(p.len - 1));
                p.pos_r = p.pos;
            }
            (LoopState::Empty, None) if rec_on => {
                p.state = LoopState::Defining;
                p.len = 0;
                p.pos = 0;
                p.pos_r = 0;
            }
            (LoopState::Defining, _) => {
                if !rec_on || p.len + 1 >= MAX_LOOP {
                    // Second press: the loop closes and the head returns home.
                    p.state = LoopState::Defined;
                    p.len = p.len.max(400);
                    p.pos = 0;
                    p.pos_r = 0;
                    p.q = 0;
                }
            }
            (LoopState::Defined, Some(n)) => {
                let n = n.clamp(400, MAX_LOOP);
                if n != p.len {
                    p.len = n;
                    p.pos %= n;
                    p.pos_r %= n;
                    p.q = 0;
                }
            }
            _ => {}
        }
    }

    pub(super) fn pulsar(&mut self, program: usize, k: &Knobs, active: bool, sync_len: Option<usize>) -> (f32, f32) {
        self.chaos_lfo(2);
        self.pulsar_transport(k.rec_on, sync_len);
        let kind = kind(program);
        let st = self.pulsar.state;
        if st == LoopState::Empty {
            let m = k.mix;
            let (l, r) = ((1.0 - m) * self.dry[0], (1.0 - m) * self.dry[1]);
            let (hl, hr) = self.halo.tick(l, r, k.blur);
            return self.halo_mix(l, r, hl, hr, k.blur);
        }
        if st == LoopState::Defining {
            // The loop grows while it records.
            self.pulsar.len += 1;
        }
        let n = self.pulsar.len.max(1);

        // ---- where each channel reads (and writes) this tick -----------------
        let (il, ir, wl, wr, gain, smooth) = self.pulsar_heads(kind, n, k);

        let (mut a, mut b) = (self.mem[il] * wl * gain, self.mem[RB + ir] * wr * gain);
        if smooth {
            // Kepler smooths the stepped reads with a two-tap average.
            self.pulsar.sm[0] = 0.5 * self.pulsar.sm[0] + 0.5 * a;
            self.pulsar.sm[1] = 0.5 * self.pulsar.sm[1] + 0.5 * b;
            a = self.pulsar.sm[0];
            b = self.pulsar.sm[1];
        }
        if let Kind::Supernova(_) = kind {
            let straight_l = self.mem[self.pulsar.pos];
            let straight_r = self.mem[RB + self.pulsar.pos];
            let t = self.pulsar.q as f32 / (n / self.slices(kind)).max(1) as f32;
            a = Self::supernova_event(self.pulsar.ev_l, a, straight_l, t);
            b = Self::supernova_event(self.pulsar.ev_r, b, straight_r, t);
        }
        self.peak = self.peak.max(a.abs()).max(b.abs());

        let (fba, fbb) = self.fb_groups(k.rec_on);
        let ya = self.tone(0, a, &k.tone) * fba * self.sup[0];
        let yb = self.tone(1, b, &k.tone) * fbb * self.sup[1];
        let in_a = self.dry[0] * self.rec_eff[0];
        let in_b = self.dry[1] * self.rec_eff[1];
        let ga = self.comp(0, in_a + ya, k);
        let gb = self.comp(1, in_b + yb, k);
        let (ya, yb) = (ya * ga, yb * gb);
        let m = k.mix;
        let (wa, wb) = (ya * m, yb * m);
        // DRIFT pans in the looper and Roche; it chooses pitch in Kepler and
        // octave drops in Supernova.
        let d = match kind {
            Kind::Loop | Kind::Resonance(..) | Kind::Roche(_) => k.drift,
            _ => 0.0,
        };
        let (p1, p2) = (self.lfo[0], self.lfo[1]);
        let ol = (wa * (1.0 - p1 * d) + wb * p2 * d) * k.fade + (1.0 - m) * self.dry[0] * ga;
        let or = (wb * (1.0 - p2 * d) + wa * p1 * d) * k.fade + (1.0 - m) * self.dry[1] * gb;
        let (ol, or) = (drive_shape(ol, k.kd) * k.post(), drive_shape(or, k.kd) * k.post());

        if active {
            if !self.freeze {
                // Roche writes back where it read; the others at the head.
                let (tl, tr, win) = match kind {
                    Kind::Roche(_) => (il, ir, wl),
                    Kind::Resonance(..) => (self.pulsar.pos, self.pulsar.pos_r, 1.0),
                    _ => (self.pulsar.pos, self.pulsar.pos, 1.0),
                };
                let w = self.saturate(0, ya * k.fb + in_a * ga * win, k);
                self.mem[tl] = w;
                let w = self.saturate(1, yb * k.fb + in_b * gb * win, k);
                self.mem[RB + tr] = w;
            }
            self.pulsar_advance(kind, n);
        }
        let (hl, hr) = self.halo.tick(ol, or, k.blur);
        self.halo_mix(ol, or, hl, hr, k.blur)
    }

    #[inline]
    fn halo_mix(&self, l: f32, r: f32, hl: f32, hr: f32, b: f32) -> (f32, f32) {
        let lift = 1.0 + b.min(1.0 - b);
        (((1.0 - b) * l + hl * b * 1.6) * lift, ((1.0 - b) * r + hr * b * 1.6) * lift)
    }

    fn slices(&self, kind: Kind) -> usize {
        match kind {
            Kind::Roche(s) | Kind::Supernova(s) => s,
            _ => 1,
        }
    }

    /// Read indices, edge windows, gain and smoothing for this tick.
    fn pulsar_heads(&mut self, kind: Kind, n: usize, k: &Knobs) -> (usize, usize, f32, f32, f32, bool) {
        let p = self.pulsar;
        let pos = p.pos.min(n - 1);
        match kind {
            Kind::Loop => (pos, pos, edge_loop(pos, n), edge_loop(pos, n), 1.0, false),
            Kind::Resonance(num, den) => {
                let nr = (n * num / den).max(1);
                let pr = p.pos_r.min(nr - 1);
                (pos, pr, 1.0, edge_loop(pr, nr), 1.0, false)
            }
            Kind::Roche(div) => {
                let slice = (n / div).max(1);
                let q = p.q.min(slice - 1);
                let w = edge(q, slice);
                ((p.src_l * slice + q).min(n - 1), (p.src_r * slice + q).min(n - 1), w, w, 1.0, false)
            }
            Kind::Supernova(div) => {
                let slice = (n / div).max(1);
                let q = p.q.min(slice - 1);
                let rep = p.rep.clamp(1, slice);
                let mut r = q % rep;
                if p.reverse {
                    r = rep - 1 - r;
                }
                if p.up {
                    r = (r * 2) % rep;
                } else if p.down {
                    r /= 2;
                }
                let w = edge(q, slice) * edge(q % rep, rep);
                ((p.src_l * slice + r).min(n - 1), (p.src_r * slice + r).min(n - 1), w, w, 1.0, false)
            }
            Kind::Kepler => {
                let dr = k.drift;
                if dr >= 0.75 {
                    let mut i = pos * 2;
                    if i >= n {
                        i -= n;
                    }
                    let w = edge_loop(i, n) * edge_loop(pos, n);
                    (i, i, w, w, 0.75, true)
                } else if dr < 0.25 {
                    let i = pos / 2;
                    let w = edge_loop(pos, n);
                    (i, i, w, w, 0.75, true)
                } else {
                    (pos, pos, 1.0, 1.0, 1.0, false)
                }
            }
        }
    }

    /// The crazy mangler's per-slice event, from its 6-bit code.
    #[inline]
    fn supernova_event(code: u32, mangled: f32, straight: f32, t: f32) -> f32 {
        match code {
            0..=3 => 0.0,                                            // drop-out
            21..=26 => (1.0 - t) * mangled + t * straight,           // dissolve back
            41..=46 => (1.0 - t) * straight + t * mangled,           // swell in
            58..=63 => straight * 0.7,                               // a pale echo
            _ => mangled,
        }
    }

    fn pulsar_advance(&mut self, kind: Kind, n: usize) {
        let rev = self.rev;
        let step = |p: usize, m: usize| -> usize {
            if rev {
                if p == 0 { m - 1 } else { p - 1 }
            } else if p + 1 >= m {
                0
            } else {
                p + 1
            }
        };
        if self.pulsar.state == LoopState::Defining {
            // While defining, the head simply follows the growing loop.
            self.pulsar.pos = (self.pulsar.len).min(MEM / 2 - 2);
            self.pulsar.pos_r = self.pulsar.pos;
            return;
        }
        self.pulsar.pos = step(self.pulsar.pos.min(n - 1), n);
        if let Kind::Resonance(num, den) = kind {
            let nr = (n * num / den).max(1);
            self.pulsar.pos_r = step(self.pulsar.pos_r.min(nr - 1), nr);
        } else {
            self.pulsar.pos_r = self.pulsar.pos;
        }
        let div = self.slices(kind);
        if div > 1 {
            let slice = (n / div).max(1);
            let q = self.pulsar.q;
            let wrapped = if rev { q == 0 } else { q + 1 >= slice };
            self.pulsar.q = if wrapped {
                if rev { slice - 1 } else { 0 }
            } else if rev {
                q - 1
            } else {
                q + 1
            };
            if let Kind::Supernova(_) = kind
                && self.pulsar.ratchet
                && self.pulsar.rep > 400
            {
                // The ratchet: each repeat a little shorter than the last.
                self.pulsar.rep -= 1;
            }
            if wrapped {
                self.new_slice(kind, div, slice);
            }
        }
        self.pulsar.flash *= 0.9995;
    }

    fn new_slice(&mut self, kind: Kind, div: usize, slice: usize) {
        let x = xorshift(&mut self.pulsar.rng) ^ self.rng;
        self.pulsar.src_l = ((x & 0xffff) >> 13) as usize % div;
        self.pulsar.src_r = (x >> 29) as usize % div;
        if let Kind::Supernova(_) = kind {
            let y = xorshift(&mut self.pulsar.rng);
            let z = y ^ (y << 16);
            self.pulsar.rep = slice >> ((y ^ (y >> 7)).wrapping_add(2) & 3);
            self.pulsar.reverse = (z >> 28) >= 14;
            let codes = z ^ (y >> 10);
            self.pulsar.ev_l = codes & 0x3f;
            self.pulsar.ev_r = (codes & 0xfff) >> 6;
            let mix = self.pulsar.ev_l ^ self.pulsar.ev_r;
            self.pulsar.up = mix & 7 == 6;
            self.pulsar.ratchet = (mix & 0x3f) >> 3 == 6;
            self.pulsar.down = self.k_drift >= 0.5 && ((z & 0x0fff_ffff) >> 20) <= 11;
            if self.pulsar.ev_l < 4 || self.pulsar.up || self.pulsar.ratchet || self.pulsar.reverse {
                self.pulsar.flash = 1.0;
            }
        }
    }

    pub(super) fn pulsar_clear(&mut self) {
        self.pulsar.state = LoopState::Empty;
        self.pulsar.len = 0;
        self.pulsar.pos = 0;
        self.pulsar.pos_r = 0;
        self.pulsar.q = 0;
        self.halo.clear();
    }

    /// Re-align the loop head with the host's bar (on transport start).
    pub fn pulsar_align(&mut self, phase: f64) {
        let p = &mut self.pulsar;
        if p.state == LoopState::Defined && p.len > 0 {
            p.pos = ((phase.rem_euclid(1.0) * p.len as f64) as usize).min(p.len - 1);
            p.pos_r = p.pos;
            p.q = 0;
        }
    }

    /// Glue for the family's layout: ramps at the loop ends of both halves.
    pub(super) fn pulsar_glue(&mut self) {
        let n = if self.pulsar.state == LoopState::Empty { RB } else { self.pulsar.len.clamp(1, RB) };
        for base in [0, RB] {
            for i in 0..super::GLUE.min(n) {
                let w = i as f32 / super::GLUE as f32;
                self.mem[base + i] *= w;
                self.mem[base + n - 1 - i] *= w;
            }
        }
    }
}

#[inline]
fn edge_loop(i: usize, n: usize) -> f32 {
    // The looper's own seam: 200-sample fades only on very short loops'
    // ends would be audible, so the firmware leaves long loops untouched.
    if n < 4 * EDGE { edge(i, n) } else { 1.0 }
}
