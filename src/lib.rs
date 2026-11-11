//! Sideral — a memory that drifts like the sky.
//!
//! The DSP is a transcription of the five COSMOS firmwares (main, quadro,
//! pitch, rhythmic and rhythmic-pitch), recovered from the decompiled
//! STM32F446 images: one 2^21-float memory shared by twenty-one programs
//! whose delay lines are sized in prime numbers of samples, asynchronous
//! LFOs whose rates wander with a Brownian "chaos" signal, slice manglers
//! that rewrite the memory in place, and the original's anti-click
//! machinery (seam ramps, cross-fades, notch "glue").
//!
//! Around that core sit things only a computer can do: loops locked to the
//! host's bars, a lossless freeze, stereo parallax and a variable engine
//! clock coupled to the host through band-limited resampling.

pub mod clock;
pub mod engine;
pub mod scope;
mod ui;

use std::sync::Arc;
use std::sync::atomic::Ordering::Relaxed;
use truce::prelude::*;

use clock::Clock;
use engine::pulsar::MAX_LOOP;
use engine::{Controls, Engine, layouts};
use scope::{BINS, Scope, Slot};

#[derive(ParamEnum)]
pub enum Program {
    #[name = "Binary I"]
    BinaryI,
    #[name = "Binary II"]
    BinaryIi,
    #[name = "Binary III"]
    BinaryIii,
    #[name = "Trapezium I"]
    TrapeziumI,
    #[name = "Trapezium II"]
    TrapeziumIi,
    #[name = "Trapezium III"]
    TrapeziumIii,
    #[name = "Nebula I"]
    NebulaI,
    #[name = "Nebula II"]
    NebulaIi,
    #[name = "Nebula III"]
    NebulaIii,
    #[name = "Stardust I"]
    StardustI,
    #[name = "Stardust II"]
    StardustIi,
    #[name = "Stardust III"]
    StardustIii,
    #[name = "Pulsar"]
    Pulsar,
    #[name = "Resonance 8:7"]
    Res87,
    #[name = "Resonance 4:3"]
    Res43,
    #[name = "Roche 8"]
    Roche8,
    #[name = "Roche 6"]
    Roche6,
    #[name = "Roche 5"]
    Roche5,
    #[name = "Supernova 8"]
    Nova8,
    #[name = "Supernova 6"]
    Nova6,
    #[name = "Kepler"]
    Kepler,
}

#[derive(ParamEnum)]
pub enum Constellation {
    #[name = "Primes I"]
    Main,
    #[name = "Primes II"]
    Quad,
}

#[derive(ParamEnum)]
pub enum Hpf {
    #[name = "Off"]
    Off,
    #[name = "266 Hz"]
    Low,
    #[name = "462 Hz"]
    Mid,
    #[name = "786 Hz"]
    High,
}

#[derive(ParamEnum)]
pub enum Lpf {
    #[name = "Off"]
    Off,
    #[name = "622 Hz"]
    Low,
    #[name = "1.2 kHz"]
    Mid,
    #[name = "5.2 kHz"]
    High,
}

#[derive(ParamEnum)]
pub enum Record {
    #[name = "Off"]
    Off,
    #[name = "Left"]
    First,
    #[name = "Right"]
    Second,
    #[name = "Both"]
    All,
}

#[derive(ParamEnum)]
pub enum Gate {
    #[name = "Open"]
    Off,
    #[name = "-48 dB"]
    Normal,
    #[name = "-68 dB"]
    Sensitive,
}

#[derive(ParamEnum)]
pub enum Period {
    #[name = "Free"]
    Free,
    #[name = "1 bar"]
    One,
    #[name = "2 bars"]
    Two,
    #[name = "4 bars"]
    Four,
    #[name = "8 bars"]
    Eight,
}

#[derive(Params)]
pub struct SideralParams {
    #[param(name = "Program", short_name = "Prog", default = 0)]
    pub program: EnumParam<Program>,
    #[param(name = "Primes", default = 0)]
    pub constellation: EnumParam<Constellation>,

    #[param(name = "Scatter", range = "linear(0, 1)", default = 0.25, unit = "%")]
    pub blur: FloatParam,
    #[param(name = "Drift", range = "linear(0, 1)", default = 0.35, unit = "%")]
    pub drift: FloatParam,
    #[param(name = "Flare", range = "linear(0, 1)", default = 0.0, unit = "%")]
    pub drive: FloatParam,
    #[param(name = "Tide", range = "linear(0, 1)", default = 0.5, format = "format_supcom")]
    pub supcom: FloatParam,
    #[param(name = "Orbit", range = "linear(0, 1)", default = 0.9, format = "format_fb")]
    pub feedback: FloatParam,
    #[param(name = "Mix", range = "linear(0, 1)", default = 0.5, unit = "%")]
    pub mix: FloatParam,

    #[param(name = "Input", short_name = "In", range = "linear(-24, 24)", default = 0.0, unit = "dB", smooth = "exp(20)")]
    pub input: FloatParam,
    #[param(name = "Output", short_name = "Out", range = "linear(-24, 12)", default = 0.0, unit = "dB", smooth = "exp(20)")]
    pub output: FloatParam,

    #[param(name = "Low Cut", default = 0)]
    pub hpf: EnumParam<Hpf>,
    #[param(name = "High Cut", default = 0)]
    pub lpf: EnumParam<Lpf>,
    #[param(name = "Rec", default = 3)]
    pub record: EnumParam<Record>,
    #[param(name = "Retrograde", short_name = "Retro", default = false)]
    pub reverse: BoolParam,
    #[param(name = "Erode", default = false)]
    pub erase: BoolParam,
    #[param(name = "Big Crunch", short_name = "Crunch", default = false)]
    pub erase_all: BoolParam,
    #[param(name = "Redshift", default = false)]
    pub half_speed: BoolParam,
    #[param(name = "Eclipse", default = false)]
    pub eclipse: BoolParam,

    #[param(name = "Dilation", range = "linear(0.5, 2)", default = 1.0, format = "format_clock")]
    pub clock: FloatParam,
    #[param(name = "Dust", default = false)]
    pub grit: BoolParam,
    #[param(name = "Parallax", range = "linear(0, 2)", default = 1.0, format = "format_width")]
    pub width: FloatParam,
    #[param(name = "Period", default = 0)]
    pub period: EnumParam<Period>,
    #[param(name = "Horizon", default = 2)]
    pub gate: EnumParam<Gate>,
    #[param(name = "Mono In", short_name = "Mono", default = false)]
    pub mono: BoolParam,

    #[skip]
    pub scope: Arc<Scope>,
}

impl SideralParams {
    fn format_fb(&self, v: f64) -> String {
        format!("×{:.3}", engine::fb_gain(v as f32))
    }
    fn format_supcom(&self, v: f64) -> String {
        let d = (v - 0.5) * 2.0;
        if d.abs() < 0.01 {
            "Still".to_owned()
        } else if d < 0.0 {
            format!("Ebb {:.0}%", d * d * 100.0)
        } else {
            format!("Flow {:.0}%", d * d * 100.0)
        }
    }
    fn format_clock(&self, v: f64) -> String {
        format!("×{v:.2}")
    }
    fn format_width(&self, v: f64) -> String {
        format!("{:.0}%", v * 100.0)
    }
}

pub struct Sideral;

pub struct SideralDsp {
    engine: Engine,
    clock: Clock,
    last_erase_all: bool,
    erase_all_seq: u32,
    was_playing: bool,
    scan_bin: usize,
    out_peak: f32,
}

impl Default for SideralDsp {
    fn default() -> Self {
        Self {
            engine: Engine::new(),
            clock: Clock::new(),
            last_erase_all: false,
            erase_all_seq: 0,
            was_playing: false,
            scan_bin: 0,
            out_peak: 0.0,
        }
    }
}

fn controls(p: &SideralParams) -> Controls {
    Controls {
        blur: p.blur.value(),
        drift: p.drift.value(),
        drive: p.drive.value(),
        supcom: p.supcom.value(),
        fb: p.feedback.value(),
        mix: p.mix.value(),
        algorithm: p.program.value().to_index(),
        quad: matches!(p.constellation.value(), Constellation::Quad),
        hpf: p.hpf.value().to_index() as u8,
        lpf: p.lpf.value().to_index() as u8,
        record: p.record.value().to_index() as u8,
        reverse: p.reverse.value(),
        erase: p.erase.value(),
        half_speed: p.half_speed.value(),
        gate: p.gate.value().to_index() as u8,
        mono: p.mono.value(),
        eclipse: p.eclipse.value(),
        width: p.width.value(),
        sync_len: None,
        sync_phase: None,
    }
}

/// Loop length (engine ticks) and bar phase from the host transport.
fn period(p: &SideralParams, t: &TransportInfo, speed: f64) -> (Option<usize>, Option<f64>) {
    let bars = match p.period.value() {
        Period::Free => return (None, None),
        Period::One => 1.0,
        Period::Two => 2.0,
        Period::Four => 4.0,
        Period::Eight => 8.0,
    };
    if t.tempo.is_nan() || t.tempo <= 1.0 {
        return (None, None);
    }
    let num = f64::from(t.time_sig_num.max(1));
    let den = f64::from(t.time_sig_den.max(1));
    let bar_q = num * 4.0 / den;
    let rate = layouts::HW_RATE * speed;
    let mut span = bars * bar_q;
    // Longer than the memory allows: halve until it fits (the firmware's
    // 898 000-sample ceiling).
    while span * 60.0 / t.tempo * rate > MAX_LOOP as f64 && span > bar_q / 4.0 {
        span *= 0.5;
    }
    let len = (span * 60.0 / t.tempo * rate) as usize;
    let phase = (t.position_beats / span).rem_euclid(1.0);
    (Some(len), Some(phase))
}

impl PluginLogic for Sideral {
    type Params = SideralParams;
    type DspState = SideralDsp;

    fn reset(state: &mut Self::DspState, params: &Self::Params, config: &AudioConfig) {
        state.clock.set_rate(config.sample_rate);
        state.engine.prime(&controls(params));
        state.engine.seed(Arc::as_ptr(&params.scope) as usize as u32);
        state.erase_all_seq = params.scope.erase_all.load(Relaxed);
    }

    fn latency(state: &Self::DspState) -> u32 {
        state.clock.latency()
    }

    fn tail(_state: &Self::DspState) -> u32 {
        // The memory can hold forever (unity orbit, or an eclipse).
        u32::MAX
    }

    fn process(
        state: &mut Self::DspState,
        params: &Self::Params,
        buffer: &mut AudioBuffer,
        _events: &EventList,
        context: &mut ProcessContext,
    ) -> ProcessStatus {
        let mut c = controls(params);
        let scope = &params.scope;
        let speed = f64::from(params.clock.value());
        let (len, phase) = period(params, &context.transport, speed);
        c.sync_len = len;
        c.sync_phase = phase;
        let playing = context.transport.playing;
        if playing
            && !state.was_playing
            && let Some(ph) = phase
        {
            state.engine.pulsar_align(ph);
        }
        state.was_playing = playing;

        // BIG CRUNCH: parameter rising edge or the editor's gesture.
        let ea = params.erase_all.value();
        let seq = scope.erase_all.load(Relaxed);
        if (ea && !state.last_erase_all) || seq != state.erase_all_seq {
            state.engine.erase_all();
        }
        state.last_erase_all = ea;
        state.erase_all_seq = seq;

        let grit = params.grit.value();
        let mono_out = buffer.num_output_channels() == 1;

        let SideralDsp { engine, clock, out_peak, .. } = state;
        let mut peak = 0.0f32;
        buffer.for_each_frame_io::<2, 2, _>(|fin, fout| {
            let gi = db(params.input.read());
            let go = db(params.output.read());
            let y = clock.process([fin[0] * gi, fin[1] * gi], speed, grit, |x| {
                let (l, r) = engine.tick(&c, x[0], x[1]);
                [l, r]
            });
            let (l, r) = (y[0] * go, y[1] * go);
            peak = peak.max(l.abs()).max(r.abs());
            if mono_out {
                fout[0] = 0.5 * (l + r);
            } else {
                fout[0] = l;
                fout[1] = r;
            }
        });
        *out_peak = peak.max(*out_peak * 0.85);

        publish(state, scope, speed as f32, c.half_speed);
        ProcessStatus::Normal
    }

    fn editor(params: Arc<SideralParams>) -> Box<dyn Editor> {
        ui::create_editor(params)
    }
}

/// Telemetry and a slice of the memory envelope for the editor.
fn publish(state: &mut SideralDsp, scope: &Scope, speed: f32, redshift: bool) {
    let t = state.engine.telemetry();
    state.engine.reset_meters();
    let b = |v: bool| if v { 1.0 } else { 0.0 };
    scope.put(Slot::Algorithm, t.algorithm as f32);
    scope.put(Slot::Quad, b(t.quad));
    let decay = |s: Slot, v: f32| scope.put(s, v.max(scope.read(s) * 0.9));
    decay(Slot::MemoryPeak, t.memory_peak);
    decay(Slot::InputPeak, t.input_peak);
    scope.put(Slot::OutputPeak, state.out_peak);
    scope.put(Slot::Suppress, t.suppress);
    decay(Slot::Limiter, t.limiter);
    scope.put(Slot::Switching, b(t.switching));
    scope.put(Slot::Settle, t.settle);
    scope.put(Slot::Clearing, b(t.clearing));
    scope.put(Slot::Reverse, t.reverse_fade);
    for i in 0..4 {
        scope.set(Slot::Lfo0 as usize + i, t.lfo[i]);
    }
    scope.put(Slot::Chaos0, t.chaos[0]);
    scope.put(Slot::Chaos1, t.chaos[1]);
    scope.put(Slot::Speed, speed);
    for (i, h) in t.heads.iter().enumerate() {
        scope.set(Slot::Head0 as usize + i, *h);
    }
    for (i, g) in t.grain_heads.iter().enumerate() {
        scope.set(Slot::Grain0 as usize + i, *g);
    }
    scope.put(Slot::LoopState, f32::from(t.loop_state));
    scope.put(Slot::LoopLen, t.loop_len as f32);
    scope.put(Slot::Slices, t.slices as f32);
    scope.put(Slot::SliceSrcL, t.slice_src[0] as f32);
    scope.put(Slot::SliceSrcR, t.slice_src[1] as f32);
    scope.put(Slot::SliceQ, t.slice_q);
    decay(Slot::Flash, t.flash);
    scope.put(Slot::Eclipse, b(t.eclipse));
    scope.put(Slot::Redshift, b(redshift));
    // Refresh 8 bins (32 768 samples) of the memory picture per block.
    let per = layouts::MEM / BINS;
    for _ in 0..8 {
        let bin = state.scan_bin;
        let chunk = &state.engine.mem[bin * per..(bin + 1) * per];
        let mut m = 0.0f32;
        for x in chunk.iter().step_by(4) {
            m = m.max(x.abs());
        }
        scope.set_bin(bin, m);
        state.scan_bin = (bin + 1) % BINS;
    }
}

#[inline]
fn db(v: f32) -> f32 {
    10.0f32.powf(v * 0.05)
}

truce::plugin! {
    logic: Sideral,
    params: SideralParams,
}

truce::enable_rt_paranoid!();
