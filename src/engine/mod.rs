//! The COSMOS engine, transcribed from the firmware's TIM2 interrupt
//! (`0x080201C4`, one call per sample at 46 875 Hz) and the twelve algorithm
//! routines it dispatches to.
//!
//! The structure follows the original closely: a global section (noise gate,
//! knob smoothing, buttons, suppressor / compressor envelopes, the reverse
//! cross-fade and the algorithm-switch state machine) followed by one of
//! four algorithm families working on a single 2^21-float memory. Field
//! comments give the RAM address each variable occupied on the hardware, so
//! the code can be checked against the decompilation in
//! `reverse_engineering/`.

pub mod layouts;
pub mod pulsar;

use layouts::{Family, Grains, Layout, MAIN, MEM, PULSAR, QUAD};
use pulsar::{Halo, LoopState, PulsarState};

/// Per-tick control inputs (knob positions and button states), already in
/// hardware units: knobs are 0..1 positions, buttons are booleans / modes.
#[derive(Clone, Copy, Debug)]
pub struct Controls {
    pub blur: f32,
    pub drift: f32,
    pub drive: f32,
    /// SUP|COM: 0 = full suppressor, 0.5 = off, 1 = full compressor.
    pub supcom: f32,
    /// FB knob position (0..1). Unity sits near 0.95, as on the panel.
    pub fb: f32,
    pub mix: f32,
    /// Panel position 0..11.
    pub algorithm: usize,
    /// Use the quadro firmware's constellation of primes.
    pub quad: bool,
    /// 0 = off, 1..3 = cutoff (1 is the lowest cutoff).
    pub hpf: u8,
    /// 0 = off, 1..3 = cutoff (1 is the lowest cutoff, the darkest).
    pub lpf: u8,
    /// 0 = off, 1 = first group (left), 2 = second group (right), 3 = all.
    pub record: u8,
    pub reverse: bool,
    pub erase: bool,
    /// The "hold RECORD five seconds" half-speed mode (delay algorithms).
    pub half_speed: bool,
    /// 0 = off, 1 = normal (0.004), 2 = sensitive (0.0004).
    pub gate: u8,
    pub mono: bool,
    /// ECLIPSE: nothing is written; the memory holds bit-exact.
    pub eclipse: bool,
    /// PARALLAX: stereo width of the output (0 mono, 1 as is, 2 wide).
    pub width: f32,
    /// Pulsar loop length locked to the host (engine ticks), or free.
    pub sync_len: Option<usize>,
    /// Host bar phase (0..1) used to place a new synced loop.
    pub sync_phase: Option<f64>,
}

impl Default for Controls {
    fn default() -> Self {
        Self {
            blur: 0.0,
            drift: 0.0,
            drive: 0.0,
            supcom: 0.5,
            fb: 0.9,
            mix: 0.5,
            algorithm: 0,
            quad: false,
            hpf: 0,
            lpf: 0,
            record: 3,
            reverse: false,
            erase: false,
            half_speed: false,
            gate: 1,
            mono: false,
            eclipse: false,
            width: 1.0,
            sync_len: None,
            sync_phase: None,
        }
    }
}

/// HPF coefficient / make-up gain for the three hardware settings.
const HPF_COEF: [(f32, f32); 3] = [(0.035, 1.035), (0.06, 1.04), (0.1, 1.063)];
/// LPF coefficient / gain / quadratic term (an even-order "tape" bend).
const LPF_COEF: [(f32, f32, f32); 3] = [(0.08, 1.033, 0.03), (0.15, 1.01, 0.025), (0.5, 1.001, 0.001)];

/// Asynchronous triangle LFO increments per tick (2D/4D/granular).
const LFO_INC: [f32; 4] = [3.1151e-6, 3.7667e-6, 2.3159e-6, 2.9663e-6];

/// Length of the linear seam ramps cut at every glue point (2.56 ms).
pub const GLUE: usize = 120;
/// Ticks the selector must rest on a new position before the switch starts
/// (the rotary switch debounce: ~1.0 s).
const SETTLE_TICKS: u32 = 47_000;
/// Ticks of dry-only output while ERASE ALL wipes the memory.
const CLEAR_CHUNK: usize = 256;

#[inline]
fn drive_shape(x: f32, k: f32) -> f32 {
    // (1+k)x − k·x|x|: odd-order, a guitar-pedal-like bend.
    let q = x * x * k;
    if x > 0.0 { (1.0 + k) * x - q } else { (1.0 + k) * x + q }
}

#[inline]
fn tail_window(i: usize, n: usize) -> f32 {
    if i < 200 {
        i as f32 / 200.0
    } else if i + 200 >= n {
        (n - 1 - i) as f32 / 200.0
    } else {
        1.0
    }
}

#[derive(Clone, Copy, Default)]
struct Tone {
    hc: f32,
    hg: f32,
    hon: f32,
    lc: f32,
    lg: f32,
    lq: f32,
    lon: f32,
}

/// What the editor shows of the engine (read once per block).
#[derive(Clone, Copy, Default, Debug)]
pub struct Telemetry {
    pub algorithm: usize,
    pub quad: bool,
    /// Normalised write-head positions of up to 9 pointer slots.
    pub heads: [f32; 9],
    pub lfo: [f32; 4],
    pub chaos: [f32; 2],
    /// Peak of what the delays are playing (drives the 5-LED meter).
    pub memory_peak: f32,
    pub input_peak: f32,
    pub suppress: f32,
    pub limiter: f32,
    pub switching: bool,
    pub settle: f32,
    pub clearing: bool,
    pub reverse_fade: f32,
    pub grain_heads: [f32; 12],
    pub loop_state: u8,
    pub loop_len: usize,
    pub slices: usize,
    pub slice_src: [usize; 2],
    pub slice_q: f32,
    pub flash: f32,
    pub eclipse: bool,
}

pub struct Engine {
    pub mem: Vec<f32>,

    // --- global / transitions --------------------------------------------
    /// @2000027c: global gain, ramps ±0.001 per tick around algorithm switches.
    gain: f32,
    /// @200003f0: settle counter of the selector.
    settle: u32,
    requested: (usize, bool),
    active: (usize, bool),
    /// @20000278 / @200001d4: reverse cross-fade and its phase.
    fade: f32,
    fading_in: bool,
    /// @20000404: current direction (true = reverse).
    rev: bool,
    /// Erase-all progress (index of next sample to clear).
    clearing: Option<usize>,
    /// @20000514 / @20000518: noise gate gains.
    gate_g: [f32; 2],

    // --- smoothed controls ------------------------------------------------
    k_blur: f32,  // @20000464
    k_drift: f32, // @20000468
    k_drive: f32, // @2000046c
    k_sup: f32,   // @20000470
    k_fb: f32,    // @20000474
    fb_gain: f32, // @20000478
    k_mix1: f32,  // @2000051c
    k_mix: f32,   // @2000047c
    keep: f32,    // @20000224 (ERASE)
    rec: [f32; 2], // @200004a8 / @200004ac
    hp_amt: f32,  // @20000498
    lp_amt: f32,  // @2000049c
    hp_mode: usize,
    lp_mode: usize,

    // --- suppressor / compressor -----------------------------------------
    env_fast: [f32; 2], // @200004e8 / @200004ec
    env_slow: [f32; 2], // @200004f0 / @200004f4
    sup: [f32; 2],      // @200004e0 / @200004e4
    comp_amt: f32,      // @200004f8

    // --- per-line state ---------------------------------------------------
    hp: [f32; 4],  // @200004c0..cc
    lp: [f32; 4],  // @200004b0..bc
    lim: [f32; 4], // @200004d0..dc
    cg: [f32; 4],  // @2000025c, 260, 4fc, 500

    // --- modulation -------------------------------------------------------
    rng: u32,         // @20000220 (xorshift32, seed 1234)
    adc_noise: u32,   // stands in for the ADC LSBs mixed into the seed
    chaos: [f32; 2],  // @20000264 / @20000268
    lfo: [f32; 4],    // @2000023c..248
    lfo_dir: [f32; 4], // @2000024c..258
    rnd_hi: u32,
    rnd_lo: u32,

    // --- pointers ---------------------------------------------------------
    /// Pointer slots @20000408 .. @20000428.
    ptr: [usize; 9],
    goff: [i32; 8], // @200001d8..1f4: L and R grain offsets
    coff: [i32; 4], // @200001f8..204: central grain offsets

    // --- reverb -----------------------------------------------------------
    carry: f32,  // @20000504
    mod_a: f32,  // @20000508
    mod_b: f32,  // @2000050c
    inc_a: f32,  // @20000270
    inc_b: f32,  // @20000274

    // --- half speed -------------------------------------------------------
    half_phase: bool, // @20000210

    // --- inputs of the current tick --------------------------------------
    dry: [f32; 2],
    rec_eff: [f32; 2],

    // --- the rhythmic family ---------------------------------------------
    pub(crate) pulsar: PulsarState,
    halo: Halo,
    sync_phase: Option<f64>,
    freeze: bool,

    // --- metering ---------------------------------------------------------
    peak: f32,
    in_peak: f32,
    lim_peak: f32,
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

impl Engine {
    pub fn new() -> Self {
        Self {
            mem: vec![0.0; MEM],
            gain: 0.0,
            settle: 0,
            requested: (0, false),
            active: (0, false),
            fade: 1.0,
            fading_in: false,
            rev: false,
            clearing: None,
            gate_g: [0.0; 2],
            k_blur: 0.0,
            k_drift: 0.0,
            k_drive: 0.0,
            k_sup: 0.5,
            k_fb: 0.0,
            fb_gain: 0.0,
            k_mix1: 0.5,
            k_mix: 0.5,
            keep: 1.0,
            rec: [0.0; 2],
            hp_amt: 0.0,
            lp_amt: 0.0,
            hp_mode: 0,
            lp_mode: 1,
            env_fast: [0.0; 2],
            env_slow: [0.0; 2],
            sup: [1.0; 2],
            comp_amt: 0.0,
            hp: [0.0; 4],
            lp: [0.0; 4],
            lim: [0.0; 4],
            cg: [1.0; 4],
            rng: 1234,
            adc_noise: 0x9E37_79B9,
            chaos: [0.0; 2],
            lfo: [0.0; 4],
            lfo_dir: [1.0; 4],
            rnd_hi: 0,
            rnd_lo: 0,
            ptr: [0; 9],
            goff: [0; 8],
            coff: [0; 4],
            carry: 0.0,
            mod_a: 0.0,
            mod_b: 0.0,
            inc_a: 0.002_112,
            inc_b: -0.001_809_9,
            half_phase: true,
            dry: [0.0; 2],
            rec_eff: [0.0; 2],
            pulsar: PulsarState { rng: 0x2545_F491, ..PulsarState::default() },
            halo: Halo::new(),
            sync_phase: None,
            freeze: false,
            peak: 0.0,
            in_peak: 0.0,
            lim_peak: 0.0,
        }
    }

    /// Seed the "ADC noise" so two instances never drift in lockstep.
    pub fn seed(&mut self, s: u32) {
        self.adc_noise ^= s.wrapping_mul(0x85EB_CA6B) | 1;
    }

    /// Start with the requested algorithm already active (no settle delay).
    pub fn prime(&mut self, c: &Controls) {
        self.requested = (c.algorithm.min(20), c.quad);
        self.active = self.requested;
        self.rev = c.reverse;
        self.ptr = [0; 9];
        self.rec = [
            if c.record == 1 || c.record == 3 { 1.0 } else { 0.0 },
            if c.record == 2 || c.record == 3 { 1.0 } else { 0.0 },
        ];
        self.k_blur = c.blur;
        self.k_drift = c.drift;
        self.k_drive = c.drive;
        self.k_sup = c.supcom;
        self.k_fb = c.fb * FB_SCALE;
        self.k_mix1 = c.mix;
        self.k_mix = c.mix;
        self.fb_gain = 1.066 * self.k_fb * (2.0 - self.k_fb);
    }

    pub fn erase_all(&mut self) {
        self.clearing = Some(0);
    }

    fn layout(key: (usize, bool)) -> &'static Layout {
        if key.0 >= 12 {
            &PULSAR
        } else if key.1 {
            &QUAD[key.0]
        } else {
            &MAIN[key.0]
        }
    }

    pub fn telemetry(&self) -> Telemetry {
        let lay = Self::layout(self.active);
        let mut t = Telemetry {
            algorithm: self.active.0,
            quad: self.active.1,
            lfo: self.lfo,
            chaos: self.chaos,
            memory_peak: self.peak,
            input_peak: self.in_peak,
            suppress: 1.0 - 0.5 * (self.sup[0] + self.sup[1]).min(2.0),
            limiter: self.lim_peak,
            switching: self.requested != self.active,
            settle: (self.settle as f32 / SETTLE_TICKS as f32).min(1.0),
            clearing: self.clearing.is_some(),
            reverse_fade: self.fade,
            ..Telemetry::default()
        };
        let slot_len = |slot: usize| -> usize {
            match (lay.family, slot) {
                (Family::TwoDelays, 0) => lay.lines[0],
                (Family::TwoDelays, 1) => lay.lines[1],
                (Family::FourDelays, 0) => lay.lines[0],
                (Family::FourDelays, 1) => lay.lines[1],
                (Family::FourDelays, 3) => lay.lines[2],
                (Family::FourDelays, 4) => lay.lines[3],
                (Family::Reverb, s) if s < 8 => lay.lines[s],
                (Family::Granular, 0) => lay.lines[0],
                (Family::Pulsar, 0 | 1) => self.pulsar.len.max(1),
                (_, 2) | (Family::Reverb, 8) => lay.tail_len().max(1),
                _ => 1,
            }
        };
        for (s, h) in t.heads.iter_mut().enumerate() {
            *h = self.ptr[s] as f32 / slot_len(s) as f32;
        }
        if lay.family == Family::Pulsar {
            let p = &self.pulsar;
            let n = p.len.max(1);
            t.heads[0] = p.pos as f32 / n as f32;
            let nr = match pulsar::kind(self.active.0) {
                pulsar::Kind::Resonance(a, b) => (n * a / b).max(1),
                _ => n,
            };
            t.heads[1] = p.pos_r as f32 / nr as f32;
            t.loop_state = match p.state {
                LoopState::Empty => 0,
                LoopState::Defining => 1,
                LoopState::Defined => 2,
            };
            t.loop_len = p.len;
            t.slices = match pulsar::kind(self.active.0) {
                pulsar::Kind::Roche(d) | pulsar::Kind::Supernova(d) => d,
                _ => 0,
            };
            t.slice_src = [p.src_l, p.src_r];
            t.slice_q = if t.slices > 0 { p.q as f32 / (n / t.slices).max(1) as f32 } else { 0.0 };
            t.flash = p.flash;
        }
        t.eclipse = self.freeze;
        if let (Some(g), Family::Granular) = (lay.grains, lay.family) {
            let n = lay.lines[0] as f32;
            for k in 0..4 {
                let c = Self::grain_ctr(self.ptr[1] as i32, k, &g);
                t.grain_heads[k] = ((self.goff[k] + c) as f32 / n).fract();
                t.grain_heads[4 + k] = ((self.goff[4 + k] + c) as f32 / n).fract();
            }
            if let Some(cg) = lay.center {
                for k in 0..4 {
                    let c = Self::grain_ctr(self.ptr[3] as i32, k, &cg);
                    t.grain_heads[8 + k] = ((self.coff[k] + c) as f32 / n).fract();
                }
            }
        }
        t
    }

    pub fn reset_meters(&mut self) {
        self.peak = 0.0;
        self.in_peak = 0.0;
        self.lim_peak = 0.0;
    }

    // =====================================================================
    // One tick of the TIM2 interrupt.
    // =====================================================================
    pub fn tick(&mut self, c: &Controls, in_l: f32, in_r: f32) -> (f32, f32) {
        // ---- input: mono sum, noise gate -------------------------------
        let (mut xl, mut xr) = (in_l, in_r);
        self.freeze = c.eclipse;
        self.sync_phase = c.sync_phase;
        if c.mono {
            let s = xl + xr;
            xl = s;
            xr = s;
        }
        self.in_peak = self.in_peak.max(xl.abs()).max(xr.abs());
        if c.gate != 0 {
            let thr = if c.gate == 2 { 0.0004 } else { 0.004 };
            for (x, g) in [(&mut xl, 0usize), (&mut xr, 1usize)] {
                let gg = &mut self.gate_g[g];
                if x.abs() <= thr {
                    *gg += *gg * -1.0e-5;
                    if *gg < 1.0e-20 {
                        *gg = 0.0;
                    }
                } else {
                    *gg += (*gg - 1.0) * -0.1;
                }
                *x *= *gg;
            }
        }

        // ---- ERASE ALL: dry only while the memory is wiped --------------
        if let Some(pos) = self.clearing {
            let end = (pos + CLEAR_CHUNK).min(MEM);
            self.mem[pos..end].fill(0.0);
            if end >= MEM {
                self.clearing = None;
                self.after_erase_all();
            } else {
                self.clearing = Some(end);
            }
            let m = self.k_mix;
            return ((1.0 - m) * xl, (1.0 - m) * xr);
        }

        // ---- reverse cross-fade (1000 ticks out, flip, 1000 in) --------
        if c.reverse != self.rev || self.fading_in {
            if !self.fading_in {
                self.fade -= 0.001;
                if self.fade < 0.001 {
                    self.fading_in = true;
                    self.rev = c.reverse;
                }
            } else {
                self.fade += 0.001;
                if self.fade >= 0.9999 {
                    self.fading_in = false;
                    self.fade = 1.0;
                }
            }
        }
        let fade = self.fade;
        let inv = 1.0 - fade;

        // ---- buttons ----------------------------------------------------
        let rec_on = c.record != 0;
        let ta = if c.record == 1 || c.record == 3 { 1.0 } else { 0.0 };
        let tb = if c.record == 2 || c.record == 3 { 1.0 } else { 0.0 };
        self.rec[0] += (self.rec[0] - ta) * -0.002;
        self.rec[1] += (self.rec[1] - tb) * -0.002;
        if c.hpf != 0 {
            self.hp_mode = (c.hpf - 1).min(2) as usize;
            self.hp_amt += (self.hp_amt - 1.0) * -5.0e-5;
        } else {
            self.hp_amt += self.hp_amt * -5.0e-5;
        }
        if c.lpf != 0 {
            self.lp_mode = (c.lpf - 1).min(2) as usize;
            self.lp_amt += (self.lp_amt - 1.0) * -5.0e-5;
        } else {
            self.lp_amt += self.lp_amt * -5.0e-5;
        }
        if c.erase {
            self.keep += self.keep * -0.0005;
        } else {
            self.keep += (self.keep - 1.1) * -0.0002;
            if self.keep >= 1.000_000_1 {
                self.keep = 1.0;
            }
        }

        // ---- knobs (the firmware's own one-pole smoothing) --------------
        self.k_blur = ((self.k_blur + (self.k_blur - c.blur) * -0.0003) * fade).max(0.0);
        self.k_drift += (self.k_drift - c.drift) * -0.0003;
        self.k_drive = ((self.k_drive + (self.k_drive - c.drive) * -0.001) * fade).max(0.0);
        self.k_sup += (self.k_sup - c.supcom) * -0.001;
        self.k_fb += (self.k_fb - c.fb * FB_SCALE) * -0.001;
        self.fb_gain = inv + (2.0 - self.k_fb) * self.k_fb * 1.066 * fade;
        self.k_mix1 += (self.k_mix1 - c.mix) * -0.001;
        self.k_mix = (self.k_mix + (self.k_mix - self.k_mix1) * -0.001).clamp(0.0, 1.0);

        self.rec_eff = [self.rec[0] * fade, self.rec[1] * fade];
        let tone = {
            let (hc, hg) = HPF_COEF[self.hp_mode];
            let (lc, lg, lq) = LPF_COEF[self.lp_mode];
            Tone { hc, hg, hon: self.hp_amt * fade, lc, lg, lq, lon: self.lp_amt * fade }
        };

        // ---- SUP|COM ----------------------------------------------------
        let s = self.k_sup;
        let mut det = [
            (xl * 4.0 * self.rec[0] * fade).abs(),
            (xr * 4.0 * self.rec[1] * fade).abs(),
        ];
        if s > 0.5 {
            let q = (s - 0.5) * 2.0;
            self.comp_amt = q * q;
            det = [0.0; 2];
        } else {
            self.comp_amt = 0.0;
        }
        let q = (0.5 - s) * 2.0;
        let sup_amt = q * q * fade;
        for ch in 0..2 {
            let d = det[ch];
            let e = self.env_fast[ch];
            self.env_fast[ch] = e + (e - d) * if d <= e { -0.000_15 } else { -0.1 };
            let e = self.env_slow[ch];
            self.env_slow[ch] = e + (e - d) * if d <= e { -0.000_04 } else { -0.1 };
            let t = ((1.0 - self.env_fast[ch] * 5.0) * self.env_slow[ch]).max(0.0);
            self.sup[ch] = ((t - self.env_fast[ch]) * sup_amt + 1.0).max(0.0);
        }

        // ---- algorithm selector: settle, fade out, glue, fade in -------
        let want = (c.algorithm.min(20), c.quad);
        if want != self.requested {
            self.requested = want;
            self.settle = 0;
        }
        if self.requested == self.active {
            self.settle = 0;
            if self.gain < 0.99999 {
                self.gain = (self.gain + 0.001).min(1.0);
            }
        } else {
            self.settle += 1;
            if self.settle > SETTLE_TICKS {
                if self.gain >= 0.001 {
                    self.gain -= 0.001;
                } else {
                    self.switch_algorithm();
                }
            }
        }

        // ---- dry signal (input × global gain) ---------------------------
        let g = self.gain;
        self.dry = [xl * g, xr * g];

        // ---- the algorithm ----------------------------------------------
        let lay = Self::layout(self.active);
        let active_tick = if c.half_speed && matches!(lay.family, Family::TwoDelays | Family::FourDelays | Family::Pulsar) {
            self.half_phase = !self.half_phase;
            self.half_phase
        } else {
            self.half_phase = true;
            true
        };
        let k = Knobs {
            blur: self.k_blur,
            drift: self.k_drift,
            kd: self.k_drive * self.k_drive * self.k_drive,
            fb: self.fb_gain,
            mix: self.k_mix,
            fade,
            inv,
            rec_on,
            tone,
        };
        let (ol, or) = match lay.family {
            Family::TwoDelays => self.two_delays(lay, &k, active_tick),
            Family::FourDelays => self.four_delays(lay, &k, active_tick),
            Family::Reverb => self.reverb(lay, &k),
            Family::Granular => self.granular(lay, &k),
            Family::Pulsar => self.pulsar(self.active.0, &k, active_tick, c.sync_len),
        };

        // ---- PARALLAX, then the hardware's parabolic soft limiter -------
        let mid = 0.5 * (ol + or);
        let side = 0.5 * (ol - or) * c.width;
        (out_clip(mid + side) * g, out_clip(mid - side) * g)
    }

    // =====================================================================
    // Shared building blocks
    // =====================================================================

    /// Feedback multipliers for the two record groups: ERASE only bites on
    /// the groups being recorded (or on everything with RECORD off).
    #[inline]
    fn fb_groups(&self, rec_on: bool) -> (f32, f32) {
        if rec_on {
            (
                ((1.0 - self.rec_eff[0]) + self.keep).min(1.0),
                ((1.0 - self.rec_eff[1]) + self.keep).min(1.0),
            )
        } else {
            (self.keep, self.keep)
        }
    }

    #[inline]
    fn tone(&mut self, i: usize, x: f32, t: &Tone) -> f32 {
        self.hp[i] += (x - self.hp[i]) * t.hc;
        let y = (1.0 - t.hon) * x + (x - self.hp[i]) * t.hon * t.hg;
        self.lp[i] += (y - self.lp[i]) * t.lc;
        let l = self.lp[i];
        (1.0 - t.lon) * y + (l * t.lg - l * l * t.lq) * t.lon
    }

    /// The compressor gain of one line, already cross-faded with `fade`.
    #[inline]
    fn comp(&mut self, i: usize, sum: f32, k: &Knobs) -> f32 {
        let t = ((0.8 - sum * sum * 1.9) * self.comp_amt + 1.0).max(0.0);
        let g = self.cg[i];
        let coef = if t <= g { -0.01 } else { -3.0e-5 };
        let n = k.inv + (g + (g - t) * coef) * k.fade;
        self.cg[i] = n;
        n
    }

    /// The loop limiter (soft knee from 0.8, gain-reduction envelope with a
    /// 10-tick attack and 1000-tick release) followed by DRIVE.
    #[inline]
    fn saturate(&mut self, i: usize, w: f32, k: &Knobs) -> f32 {
        let a = w.abs();
        let t = if a < 0.8 {
            0.0
        } else if a < 1.2 {
            (a - 0.8) * (a - 0.8) * 1.25
        } else {
            a - 1.0
        };
        let l = self.lim[i];
        let mut l = l + (l - t) * if t <= l { -0.001 } else { -0.1 };
        if l >= 1.000_000_1 {
            l = 1.0;
        }
        self.lim[i] = l;
        self.lim_peak = self.lim_peak.max(l);
        drive_shape((1.0 - l) * w, k.kd) * k.post()
    }

    /// xorshift32 + the two Brownian "chaos" walks + `n` triangle LFOs.
    #[inline]
    fn chaos_lfo(&mut self, n: usize) {
        // The firmware folds three ADC LSBs into the seed every tick.
        self.adc_noise = self.adc_noise.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let mut x = self.rng ^ ((self.adc_noise >> 29) & 7);
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        self.rnd_hi = x >> 16;
        self.rnd_lo = x & 0xffff;
        let wander = |c: &mut f32, step: f32| {
            *c += step;
            if *c >= 1.000_000_1 {
                *c = -0.5;
            }
            if *c < -0.5 {
                *c = 1.0;
            }
        };
        wander(&mut self.chaos[0], (self.rnd_hi as i32 - 0x8000) as f32 * 5.0e-9);
        wander(&mut self.chaos[1], (self.rnd_lo as i32 - 0x8000) as f32 * 1.0e-8);
        for i in 0..n {
            let ch = self.chaos[i / 2];
            let p = self.lfo[i] + self.lfo_dir[i] * LFO_INC[i] * (ch + 1.0);
            if p < 0.0 {
                self.lfo_dir[i] = 1.0;
            }
            if p >= 1.000_000_1 {
                self.lfo_dir[i] = -1.0;
            }
            self.lfo[i] = p;
        }
    }

    #[inline]
    fn step(&self, p: usize, n: usize) -> usize {
        if self.rev {
            if p == 0 { n - 1 } else { p - 1 }
        } else if p + 1 >= n {
            0
        } else {
            p + 1
        }
    }

    // =====================================================================
    // Two delays (FUN_08024494 and siblings)
    // =====================================================================
    fn two_delays(&mut self, lay: &Layout, k: &Knobs, active: bool) -> (f32, f32) {
        self.chaos_lfo(2);
        let (na, nb) = (lay.lines[0], lay.lines[1]);
        let ia = self.ptr[0];
        let ib = na + self.ptr[1];
        let (a, b) = (self.mem[ia], self.mem[ib]);
        self.peak = self.peak.max(a.abs()).max(b.abs());
        let (fba, fbb) = self.fb_groups(k.rec_on);
        let s = 1.0 - k.blur * 0.49;
        let aa = s * a + b * k.blur * 0.51;
        let bb = s * b + a * k.blur * 0.51;
        let ya = self.tone(0, aa, &k.tone) * fba * self.sup[0];
        let yb = self.tone(1, bb, &k.tone) * fbb * self.sup[1];
        let in_a = self.dry[0] * self.rec_eff[0];
        let in_b = self.dry[1] * self.rec_eff[1];
        let ga = self.comp(0, in_a + ya, k);
        let gb = self.comp(1, in_b + yb, k);
        let (ya, yb) = (ya * ga, yb * gb);
        let (wa, wb) = (ya * k.mix, yb * k.mix);
        let (p1, p2, d) = (self.lfo[0], self.lfo[1], k.drift);
        let ol = (wa * (1.0 - p1 * d) + wb * p2 * d) * k.fade + (1.0 - k.mix) * self.dry[0] * ga;
        let or = (wb * (1.0 - p2 * d) + wa * p1 * d) * k.fade + (1.0 - k.mix) * self.dry[1] * gb;
        let out = (drive_shape(ol, k.kd) * k.post(), drive_shape(or, k.kd) * k.post());

        if active {
            if !self.freeze {
                let w = self.saturate(0, ya * k.fb + in_a * ga, k);
                self.mem[ia] = w;
                let w = self.saturate(1, yb * k.fb + in_b * gb, k);
                self.mem[ib] = w;
                self.write_tail(lay, w);
            }
            self.ptr[0] = self.step(self.ptr[0], na);
            self.ptr[1] = self.step(self.ptr[1], nb);
        }
        out
    }

    // =====================================================================
    // Four delays (FUN_08026b30 and siblings)
    // =====================================================================
    fn four_delays(&mut self, lay: &Layout, k: &Knobs, active: bool) -> (f32, f32) {
        self.chaos_lfo(4);
        let l = lay.lines;
        let ia = self.ptr[0];
        let ib = l[0] + self.ptr[1];
        let ic = l[0] + l[1] + self.ptr[3];
        let id = l[0] + l[1] + l[2] + self.ptr[4];
        let (a, b, cc, dd) = (self.mem[ia], self.mem[ib], self.mem[ic], self.mem[id]);
        self.peak = self.peak.max(a.abs()).max(b.abs()).max(cc.abs()).max(dd.abs());
        let (fbl, fbr) = self.fb_groups(k.rec_on);
        let bl = k.blur;
        let s = 1.0 - bl * 0.46;
        // The cross-feed ring A ← −D, B ← C, C ← A, D ← B.
        let a2 = s * a + dd * bl * -0.6;
        let b2 = s * b + cc * bl * 0.6;
        let c2 = s * cc + a * bl * 0.55;
        let d2 = s * dd + b * bl * 0.55;
        let ya = self.tone(0, a2, &k.tone) * fbl * self.sup[0];
        let yb = self.tone(1, b2, &k.tone) * fbr * self.sup[1];
        let yc = self.tone(2, c2, &k.tone) * fbl * self.sup[0];
        let yd = self.tone(3, d2, &k.tone) * fbr * self.sup[1];
        let in_l = self.dry[0] * self.rec_eff[0];
        let in_r = self.dry[1] * self.rec_eff[1];
        let ga = self.comp(0, in_l + ya, k);
        let gb = self.comp(1, in_r + yb, k);
        let gc = self.comp(2, in_l + yc, k);
        let gd = self.comp(3, in_r + yd, k);
        let (ya, yb, yc, yd) = (ya * ga, yb * gb, yc * gc, yd * gd);
        let m = k.mix;
        let (wa, wb, wc, wd) = (ya * m, yb * m, yc * m, yd * m);
        let d = k.drift;
        let (p1, p2, p3, p4) = (self.lfo[0], self.lfo[1], self.lfo[2], self.lfo[3]);
        let near = (1.0 - d) * 0.7;
        let far = (1.0 - d) * 0.4;
        let ol = (wa * (1.0 - p1 * d) + wb * p2 * d + wc * (near + p3 * d) + wd * (far + (1.0 - p4) * d))
            * 0.7
            * k.fade
            + (1.0 - m) * self.dry[0] * ga * gc;
        let or = (wb * (1.0 - p2 * d) + wa * p1 * d + wd * (near + p4 * d) + wc * (far + (1.0 - p3) * d))
            * 0.7
            * k.fade
            + (1.0 - m) * self.dry[1] * gb * gd;
        let out = (drive_shape(ol, k.kd) * k.post(), drive_shape(or, k.kd) * k.post());

        if active {
            if !self.freeze {
                let w = self.saturate(0, ya * k.fb + in_l * ga, k);
                self.mem[ia] = w;
                let w = self.saturate(1, yb * k.fb + in_r * gb, k);
                self.mem[ib] = w;
                let w = self.saturate(2, yc * k.fb + in_l * gc, k);
                self.mem[ic] = w;
                let w = self.saturate(3, yd * k.fb + in_r * gd, k);
                self.mem[id] = w;
                self.write_tail(lay, if lay.tail_zero { 0.0 } else { w });
            }
            self.ptr[0] = self.step(self.ptr[0], l[0]);
            self.ptr[1] = self.step(self.ptr[1], l[1]);
            self.ptr[3] = self.step(self.ptr[3], l[2]);
            self.ptr[4] = self.step(self.ptr[4], l[3]);
        }
        out
    }

    /// The tail keeps a running recording of the newest line, faded over
    /// 200 samples at both of its ends.
    #[inline]
    fn write_tail(&mut self, lay: &Layout, w: f32) {
        let n = lay.tail_len();
        if n == 0 {
            return;
        }
        let p = if self.ptr[2] < n { self.ptr[2] } else { 0 };
        self.mem[lay.tail_base() + p] = if lay.tail_zero { 0.0 } else { w * tail_window(p, n) };
        self.ptr[2] = self.step(p, n);
    }

    // =====================================================================
    // Giant reverb (FUN_0802a138 and siblings): four Schroeder all-pass +
    // delay stages in a ring, the first two all-passes read through slowly
    // swept fractional taps.
    // =====================================================================
    fn reverb(&mut self, lay: &Layout, k: &Knobs) -> (f32, f32) {
        let l = lay.lines;
        let mut base = [0usize; 9];
        for i in 1..9 {
            base[i] = base[i - 1] + l[i - 1];
        }
        let decay = k.fb * 0.91;
        let gap = (1.0 - k.fb * 0.5) * 0.55;
        let (f, inv) = if self.freeze { (0.0, 1.0) } else { (k.fade, k.inv) };
        let m = k.mix;

        // Outputs are taken from the delay taps before this tick's writes.
        let d1 = self.mem[base[1] + self.ptr[1]];
        let d2 = self.mem[base[3] + self.ptr[3]];
        let d3 = self.mem[base[5] + self.ptr[5]];
        let d4 = self.mem[base[7] + self.ptr[7]];
        let ol = (d1 + d2 * 0.7 + d4 * 0.4) * 0.7 * m * k.fade + (1.0 - m) * self.dry[0];
        let or = (d3 + d4 * 0.5 + d2 * 0.4) * 0.7 * m * k.fade + (1.0 - m) * self.dry[1];
        let out = (drive_shape(ol, k.kd) * k.post(), drive_shape(or, k.kd) * k.post());

        let keep = self.keep;
        let in_l = self.dry[0] * self.rec_eff[0];
        let in_r = self.dry[1] * self.rec_eff[1];

        // Stage 1 (left input, compressor, swept all-pass).
        let y = self.tone(0, self.carry, &k.tone) * keep * self.sup[0];
        let g = self.comp(0, in_l + y, k);
        let v = self.saturate(0, in_l + g * y, k);
        let r1 = self.swept(base[0], l[0], self.ptr[0], self.mod_a);
        let s2 = self.allpass_stage(base[0] + self.ptr[0], base[1] + self.ptr[1], v, r1, gap, f, inv) * decay;

        // Stage 2 (right input, swept all-pass).
        let y = self.tone(1, s2, &k.tone) * keep * self.sup[1];
        let v = self.saturate(1, in_r + y, k);
        let r2 = self.swept(base[2], l[2], self.ptr[2], self.mod_b);
        let s3 = self.allpass_stage(base[2] + self.ptr[2], base[3] + self.ptr[3], v, r2, gap, f, inv) * decay;

        // Stage 3 (left input).
        let y = self.tone(2, s3, &k.tone) * keep * self.sup[0];
        let v = self.saturate(2, in_l + y, k);
        let r3 = self.mem[base[4] + self.ptr[4]];
        let s4 = self.allpass_stage(base[4] + self.ptr[4], base[5] + self.ptr[5], v, r3, gap, f, inv) * decay;

        // Stage 4 (right input), closing the ring.
        let y = self.tone(3, s4, &k.tone) * keep * self.sup[1];
        let v = self.saturate(3, in_r + y, k);
        let r4 = self.mem[base[6] + self.ptr[6]];
        let s5 = self.allpass_stage(base[6] + self.ptr[6], base[7] + self.ptr[7], v, r4, gap, f, inv) * decay;
        self.carry = s5;
        if lay.tail_len() > 0 {
            if !self.freeze {
                self.mem[base[8] + self.ptr[8]] = s5;
            }
            self.ptr[8] = if self.ptr[8] + 1 >= lay.tail_len() { 0 } else { self.ptr[8] + 1 };
        }
        self.peak = self.peak.max(r1.abs()).max(r2.abs()).max(r3.abs()).max(r4.abs());

        for i in 0..8 {
            self.ptr[i] = self.step(self.ptr[i], l[i]);
        }
        self.mod_a += self.inc_a;
        if self.mod_a >= 8300.0 {
            self.inc_a = -0.002_112;
        }
        if self.mod_a < 0.0 {
            self.inc_a = 0.002_112;
            self.mod_a = 0.0;
        }
        self.mod_b += self.inc_b;
        if self.mod_b >= 11_100.0 {
            self.inc_b = -0.001_809_9;
        }
        if self.mod_b < 0.0 {
            self.inc_b = 0.001_809_9;
            self.mod_b = 0.0;
        }
        out
    }

    /// Linear-interpolated read `m` samples ahead of (or, reversed, behind)
    /// the write head of an all-pass line.
    #[inline]
    fn swept(&self, base: usize, n: usize, p: usize, m: f32) -> f32 {
        let mi = m as usize;
        let fr = m - mi as f32;
        let (i0, i1) = if self.rev {
            let i0 = (p + n - mi % n) % n;
            (i0, if i0 == 0 { n - 1 } else { i0 - 1 })
        } else {
            let i0 = (p + mi) % n;
            (i0, if i0 + 1 >= n { 0 } else { i0 + 1 })
        };
        (1.0 - fr) * self.mem[base + i0] + self.mem[base + i1] * fr
    }

    /// Schroeder all-pass write plus the delay that follows it; returns the
    /// delay's output. Writes are cross-faded with the old content so the
    /// reverse transition freezes the memory instead of muting it.
    #[inline]
    #[allow(clippy::too_many_arguments)]
    fn allpass_stage(&mut self, ap: usize, dl: usize, v: f32, r: f32, gap: f32, f: f32, inv: f32) -> f32 {
        self.mem[ap] = (v + r * gap) * f + self.mem[ap] * inv;
        let old = self.mem[dl];
        self.mem[dl] = (r - v * gap) * f + old * inv;
        old
    }

    // =====================================================================
    // Granular delay (FUN_0802cc3c and siblings)
    // =====================================================================
    #[inline]
    fn grain_ctr(master: i32, k: usize, g: &Grains) -> i32 {
        if k == 0 {
            master
        } else {
            let c = master - g.offs[k - 1];
            if c < 0 { c + g.len } else { c }
        }
    }

    #[inline]
    fn grain_window(c: i32, g: &Grains) -> f32 {
        let w = if c < g.bound { c } else { g.end - c };
        w as f32 / g.half
    }

    fn granular(&mut self, lay: &Layout, k: &Knobs) -> (f32, f32) {
        self.chaos_lfo(4);
        let n = lay.lines[0];
        let nm1 = (n - 1) as f32;
        let p1 = self.lfo[0].clamp(0.0, 1.0);
        let p2 = self.lfo[1].clamp(0.0, 1.0);
        let p3 = self.lfo[2].clamp(0.0, 1.0);
        let rec = self.rec_eff[0].max(self.rec_eff[1]);

        // The 'delay' itself: one mono line the grains feed on.
        let i0 = self.ptr[0];
        let x = self.mem[i0];
        self.peak = self.peak.max(x.abs());
        let y = self.tone(0, x, &k.tone) * self.keep * (self.sup[0] + self.sup[1]) * 0.5;
        let input = (self.dry[0] + self.dry[1]) * 0.5 * rec;
        let g = self.comp(0, input + y, k);
        let w = self.saturate(0, y * g * k.fb + input * g, k);
        if !self.freeze {
            self.mem[i0] = w;
        }

        let gr = lay.grains.expect("granular layout");
        let rh = self.rnd_hi as f32 / 65535.0;
        let rl = self.rnd_lo as f32 / 65535.0;
        let blur = k.blur;
        let drift = k.drift;
        let master = self.ptr[1] as i32;
        let (mut sl, mut sr) = (0.0f32, 0.0f32);
        for kk in 0..4 {
            let cnt = Self::grain_ctr(master, kk, &gr);
            if cnt == 0 {
                self.goff[kk] = (rh * nm1 * blur) as i32 + (p2 * nm1 * drift * 0.9) as i32;
                self.goff[4 + kk] = (p1 * nm1 * drift * 0.9) as i32 + (((rl - 0.5) * blur + 0.5) * nm1) as i32;
            }
            let win = Self::grain_window(cnt, &gr);
            sl += win * self.mem[Self::wrap_read(self.goff[kk] + cnt, n)];
            sr += win * self.mem[Self::wrap_read(self.goff[4 + kk] + cnt, n)];
        }
        let mut sc = 0.0f32;
        if let Some(cg) = lay.center {
            let cm = self.ptr[3] as i32;
            for kk in 0..4 {
                let cnt = Self::grain_ctr(cm, kk, &cg);
                if cnt == 0 {
                    self.coff[kk] = (rh * nm1 * blur) as i32 + (p3 * nm1 * drift * 0.9) as i32;
                }
                sc += Self::grain_window(cnt, &cg) * self.mem[Self::wrap_read(self.coff[kk] + cnt, n)];
            }
            sc *= 0.85;
            self.ptr[3] = self.step(self.ptr[3], cg.len as usize);
        }
        let m = k.mix;
        let gg = lay.grain_gain;
        let ol = (sc + sl) * gg * m * k.fade + (1.0 - m) * self.dry[0];
        let or = (sc + sr) * gg * m * k.fade + (1.0 - m) * self.dry[1];
        let out = (drive_shape(ol, k.kd) * k.post(), drive_shape(or, k.kd) * k.post());

        if !self.freeze {
            self.write_tail(lay, w);
        }
        self.ptr[0] = self.step(i0, n);
        self.ptr[1] = self.step(self.ptr[1], gr.len as usize);
        out
    }

    #[inline]
    fn wrap_read(i: i32, n: usize) -> usize {
        let mut i = i.max(0) as usize;
        while i >= n {
            i -= n;
        }
        i
    }

    // =====================================================================
    // Transitions
    // =====================================================================

    /// Linear seam ramps (0 → 1 over 120 samples) on both ends of every
    /// segment, plus V-shaped notches where the old write heads stood.
    fn glue(&mut self, lay: &Layout, heads: bool) {
        if lay.family == Family::Pulsar {
            self.pulsar_glue();
            return;
        }
        let mut base = 0;
        for &n in lay.lines {
            for i in 0..GLUE.min(n) {
                let w = i as f32 / GLUE as f32;
                self.mem[base + i] *= w;
                self.mem[base + n - 1 - i] *= w;
            }
            base += n;
        }
        if !heads {
            return;
        }
        match lay.family {
            Family::Reverb => {
                let mut b = 0;
                for (i, &n) in lay.lines.iter().enumerate() {
                    self.notch(b + self.ptr[i].min(n - 1));
                    b += n;
                }
                if lay.tail_len() > 0 {
                    self.notch(b + self.ptr[8].min(lay.tail_len() - 1));
                }
            }
            _ => {
                if let Some(nb) = lay.tail_notch {
                    self.notch(nb + self.ptr[2]);
                }
            }
        }
    }

    fn notch(&mut self, c: usize) {
        for i in 0..GLUE {
            let w = i as f32 / GLUE as f32;
            if c + i < MEM {
                self.mem[c + i] *= w;
            }
            if c >= i && c - i < MEM {
                self.mem[c - i] *= w;
            }
        }
    }

    fn switch_algorithm(&mut self) {
        let old = Self::layout(self.active);
        let new = Self::layout(self.requested);
        self.glue(old, true);
        self.glue(new, false);
        self.active = self.requested;
        self.settle = 0;
        self.ptr = [0; 9];
        self.cg = [1.0; 4];
        self.hp = [0.0; 4];
        self.lp = [0.0; 4];
        self.lim = [0.0; 4];
        self.carry = 0.0;
        self.mod_a = 0.0;
        self.mod_b = 0.0;
        self.inc_a = 0.001_809_9;
        self.inc_b = 0.002_112;
        self.rec_eff = [0.0; 2];
    }

    fn after_erase_all(&mut self) {
        self.hp = [0.0; 4];
        self.lp = [0.0; 4];
        self.lim = [0.0; 4];
        self.cg = [1.0; 4];
        self.carry = 0.0;
        self.mod_a = 0.0;
        self.mod_b = 0.0;
        self.inc_a = 0.001_809_9;
        self.inc_b = 0.002_112;
        self.env_fast = [0.0; 2];
        self.env_slow = [0.0; 2];
        self.gain = 9.324_634_5e-6;
        self.pulsar_clear();
    }
}

/// FB knob → the hardware's ADC-over-calibration ratio (cal = 5200).
const FB_SCALE: f32 = 4095.0 / 5200.0;

/// Output stage soft limiter: identity up to 0.9, then a parabola that
/// lands with zero slope at 0.9992 for inputs of 1.0984 and beyond.
#[inline]
pub fn out_clip(x: f32) -> f32 {
    if x >= 0.9 {
        let x = x.min(1.0984);
        x - (x - 0.9) * (x - 0.9) * 2.52
    } else if x < -0.9 {
        let x = x.max(-1.0984);
        x + (x + 0.9) * (x + 0.9) * 2.52
    } else {
        x
    }
}

/// FB knob position → loop gain, as the firmware computes it.
pub fn fb_gain(pos: f32) -> f32 {
    let k = pos * FB_SCALE;
    1.066 * k * (2.0 - k)
}

#[derive(Clone, Copy)]
struct Knobs {
    blur: f32,
    drift: f32,
    /// DRIVE³.
    kd: f32,
    fb: f32,
    mix: f32,
    fade: f32,
    inv: f32,
    rec_on: bool,
    tone: Tone,
}

impl Knobs {
    #[inline]
    fn post(&self) -> f32 {
        1.0 - self.kd * 0.3
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unity_feedback_point() {
        // Calibration stores cal·k/0.75: unity sits at ADC = 0.75·5200.
        let pos = 0.75 / FB_SCALE;
        assert!((fb_gain(pos) - 0.9994).abs() < 1e-3);
        assert!(fb_gain(1.0) > 1.0 && fb_gain(1.0) < 1.02);
    }

    #[test]
    fn out_clip_is_smooth() {
        assert!((out_clip(0.5) - 0.5).abs() < 1e-7);
        assert!((out_clip(1.0984) - 0.9992).abs() < 1e-3);
        assert!(out_clip(5.0) <= 1.0);
    }

    #[test]
    fn every_algorithm_runs_and_stays_bounded() {
        let mut e = Engine::new();
        for alg in 0..12 {
            for quad in [false, true] {
                let c = Controls { algorithm: alg, quad, fb: 1.0, blur: 0.7, drift: 0.8, drive: 0.4, ..Controls::default() };
                e.prime(&c);
                let mut peak = 0.0f32;
                for i in 0..60_000 {
                    let x = ((i as f32) * 0.05).sin() * 0.8;
                    let (l, r) = e.tick(&c, x, -x);
                    assert!(l.is_finite() && r.is_finite());
                    peak = peak.max(l.abs()).max(r.abs());
                }
                assert!(peak <= 1.0, "alg {alg} quad {quad} peak {peak}");
            }
        }
    }

    #[test]
    fn delay_returns_after_its_prime_length() {
        let mut e = Engine::new();
        let c = Controls { algorithm: 0, mix: 1.0, fb: 0.0, gate: 0, ..Controls::default() };
        e.prime(&c);
        e.gain = 1.0;
        // Impulse into line A: it must come back exactly 100 003 ticks later.
        let mut hit = None;
        for i in 0..101_010 {
            let x = if i == 1000 { 0.5 } else { 0.0 };
            let (l, _) = e.tick(&c, x, 0.0);
            if i > 1010 && l.abs() > 0.05 && hit.is_none() {
                hit = Some(i);
            }
        }
        assert_eq!(hit, Some(1000 + 100_003));
    }

    #[test]
    fn pulsar_free_loop_closes_and_repeats() {
        let mut e = Engine::new();
        let mut c = Controls { algorithm: 12, mix: 1.0, fb: 0.952, gate: 0, record: 3, ..Controls::default() };
        e.prime(&c);
        e.gain = 1.0;
        // Record 30 000 ticks with an impulse at 1000, then release ACCRETE.
        for i in 0..30_000 {
            let x = if i == 1000 { 0.5 } else { 0.0 };
            e.tick(&c, x, x);
        }
        c.record = 0;
        let mut hits = Vec::new();
        for i in 0..70_000 {
            let (l, _) = e.tick(&c, 0.0, 0.0);
            if l.abs() > 0.05 {
                hits.push(i);
            }
        }
        assert_eq!(e.pulsar.state, LoopState::Defined);
        let n = e.pulsar.len;
        assert!((29_990..30_010).contains(&n), "len {n}");
        assert!(hits.len() >= 2, "{hits:?}");
        assert_eq!(hits[1] - hits[0], n);
    }

    #[test]
    fn rhythmic_programs_stay_bounded() {
        for alg in 12..21 {
            let mut e = Engine::new();
            let c = Controls { algorithm: alg, fb: 1.0, drift: 0.9, blur: 0.6, gate: 0, sync_len: Some(40_000), ..Controls::default() };
            e.prime(&c);
            let mut peak = 0.0f32;
            for i in 0..200_000 {
                let x = ((i as f32) * 0.031).sin() * 0.7;
                let (l, r) = e.tick(&c, x, x * 0.5);
                assert!(l.is_finite() && r.is_finite(), "alg {alg}");
                peak = peak.max(l.abs()).max(r.abs());
            }
            assert!(peak <= 1.0 && peak > 0.01, "alg {alg} peak {peak}");
        }
    }

    #[test]
    fn eclipse_holds_memory_bit_exact() {
        for alg in [0, 4, 7, 10, 12, 15] {
            let mut e = Engine::new();
            let mut c = Controls { algorithm: alg, fb: 0.8, gate: 0, sync_len: Some(30_000), ..Controls::default() };
            e.prime(&c);
            for i in 0..50_000 {
                let x = ((i as f32) * 0.02).sin() * 0.5;
                e.tick(&c, x, x);
            }
            c.eclipse = true;
            let snapshot = e.mem.clone();
            for i in 0..50_000 {
                let x = ((i as f32) * 0.03).sin() * 0.5;
                e.tick(&c, x, x);
            }
            assert!(e.mem == snapshot, "alg {alg} changed under eclipse");
        }
    }
}
