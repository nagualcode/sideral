//! The engine clock.
//!
//! The COSMOS engine runs on its own clock: 46 875 Hz on the hardware, and
//! anything between half and double that here (the "pitch" firmware simply
//! reprogrammed TIM2's period to slow the whole DSP down). This module
//! couples that clock to the host's: band-limited (Kaiser-windowed sinc)
//! interpolation on the way in and on the way out, with a fixed latency so
//! the host can compensate it whatever the varispeed setting.
//!
//! With "grit" on, both conversions fall back to sample-and-hold, which is
//! what the hardware's pitch firmware actually did: the ISR read the codec
//! register at its own pace with no anti-aliasing and held its output.

use crate::engine::layouts::HW_RATE;

/// Zero crossings on each side of the kernel.
const ZC: usize = 32;
/// Table resolution per zero crossing.
const RES: usize = 256;
/// Cut-off relative to the lower of the two Nyquist frequencies.
const CUTOFF: f64 = 0.92;
const BETA: f64 = 7.5;
/// Slowest and fastest engine clock relative to 46 875 Hz.
pub const SPEED_MIN: f64 = 0.5;
pub const SPEED_MAX: f64 = 2.0;
const RING: usize = 1 << 13;
/// Fractional positions the polyphase tables are built for (plus one row,
/// so neighbouring rows can be interpolated).
const PHASES: usize = 256;
/// Rows built per call while a table is being (re)computed.
const ROWS_PER_CALL: usize = 4;
/// Calls a new bandwidth must hold still before its table is built.
const SETTLE_CALLS: u32 = 32;

fn bessel_i0(x: f64) -> f64 {
    let mut sum = 1.0;
    let mut term = 1.0;
    let q = x * x / 4.0;
    for k in 1..64 {
        term *= q / (k * k) as f64;
        sum += term;
        if term < 1e-12 * sum {
            break;
        }
    }
    sum
}

pub struct Kernel {
    table: Vec<f32>,
}

impl Kernel {
    pub fn new() -> Self {
        let n = ZC * RES + 2;
        let i0b = bessel_i0(BETA);
        let table = (0..n)
            .map(|i| {
                let u = i as f64 / RES as f64;
                if u >= ZC as f64 {
                    return 0.0;
                }
                let sinc = if u == 0.0 { 1.0 } else { (std::f64::consts::PI * u).sin() / (std::f64::consts::PI * u) };
                let r = u / ZC as f64;
                let w = bessel_i0(BETA * (1.0 - r * r).max(0.0).sqrt()) / i0b;
                (sinc * w) as f32
            })
            .collect();
        Self { table }
    }

    /// K(u) for u ≥ 0 (in zero-crossing units).
    #[inline]
    fn at(&self, u: f32) -> f32 {
        let x = u * RES as f32;
        let i = x as usize;
        if i + 1 >= self.table.len() {
            return 0.0;
        }
        let f = x - i as f32;
        self.table[i] + (self.table[i + 1] - self.table[i]) * f
    }
}

impl Default for Kernel {
    fn default() -> Self {
        Self::new()
    }
}

/// Polyphase form of the kernel for one bandwidth `s`: row `p` holds the
/// weights of every tap for the fractional position `p / PHASES`. A read
/// then is a blend of two neighbouring rows and one dot product, which the
/// compiler vectorises - the direct form walks the kernel table tap by tap.
///
/// The table follows the engine clock: when the bandwidth changes (the
/// DILATION knob moves) reads fall back to the direct form until the knob
/// has rested, then the table is rebuilt a few rows per call.
struct Phases {
    /// Bandwidth the rows were built for.
    s: f32,
    half: usize,
    /// Rows finished so far (`PHASES + 1` when usable).
    rows: usize,
    /// Row length reserved (worst-case tap count).
    stride: usize,
    data: Vec<f32>,
    /// Blended weights of the current read.
    w: Vec<f32>,
    want: f32,
    stable: u32,
}

impl Phases {
    fn new() -> Self {
        Self { s: 0.0, half: 0, rows: 0, stride: 0, data: Vec::new(), w: Vec::new(), want: 0.0, stable: 0 }
    }

    /// Reserve for bandwidths down to `s_min` (not on the audio thread).
    fn reserve(&mut self, s_min: f32) {
        self.stride = 2 * (ZC as f32 / s_min).ceil() as usize + 2;
        self.data = vec![0.0; self.stride * (PHASES + 1)];
        self.w = vec![0.0; self.stride];
        self.s = 0.0;
        self.rows = 0;
        self.stable = 0;
    }

    fn start(&mut self, s: f32) {
        self.s = s;
        self.half = (ZC as f32 / s).ceil() as usize;
        self.rows = 0;
    }

    fn build_rows(&mut self, k: &Kernel, n: usize) {
        let taps = 2 * self.half;
        if taps > self.stride {
            return;
        }
        for _ in 0..n {
            if self.rows > PHASES {
                break;
            }
            let fr = self.rows as f32 / PHASES as f32;
            let row = &mut self.data[self.rows * self.stride..self.rows * self.stride + taps];
            for (t, w) in row.iter_mut().enumerate() {
                let d = (t as i64 - self.half as i64 + 1) as f32 - fr;
                *w = k.at((d * self.s).abs()) * self.s;
            }
            self.rows += 1;
        }
    }

    /// Build the whole table at once (when the stream is prepared).
    fn prebuild(&mut self, k: &Kernel, s: f32) {
        self.start(s);
        self.build_rows(k, PHASES + 1);
        self.want = s;
    }

    /// Is the table usable for bandwidth `s`? Advances a pending build.
    #[inline]
    fn ready(&mut self, k: &Kernel, s: f32) -> bool {
        if s == self.s {
            if self.rows > PHASES {
                return true;
            }
            self.build_rows(k, ROWS_PER_CALL);
            return false;
        }
        if s == self.want {
            self.stable += 1;
            if self.stable >= SETTLE_CALLS {
                self.start(s);
            }
        } else {
            self.want = s;
            self.stable = 0;
        }
        false
    }
}

/// Dot product with eight partial sums, so it vectorises.
#[inline]
fn dot(x: &[f32], w: &[f32]) -> f32 {
    let mut acc = [0.0f32; 8];
    let (xc, wc) = (x.chunks_exact(8), w.chunks_exact(8));
    let (xt, wt) = (xc.remainder(), wc.remainder());
    for (x, w) in xc.zip(wc) {
        for j in 0..8 {
            acc[j] += x[j] * w[j];
        }
    }
    let mut sum = ((acc[0] + acc[1]) + (acc[2] + acc[3])) + ((acc[4] + acc[5]) + (acc[6] + acc[7]));
    for (x, w) in xt.iter().zip(wt) {
        sum += x * w;
    }
    sum
}

/// Stereo ring addressed by absolute sample index. Every sample is stored
/// twice, `RING` apart, so any window is one contiguous slice.
struct Ring {
    l: Vec<f32>,
    r: Vec<f32>,
    written: u64,
}

impl Ring {
    fn new() -> Self {
        Self { l: vec![0.0; RING * 2], r: vec![0.0; RING * 2], written: 0 }
    }
    fn clear(&mut self) {
        self.l.fill(0.0);
        self.r.fill(0.0);
        self.written = 0;
    }
    #[inline]
    fn push(&mut self, x: [f32; 2]) {
        let i = (self.written as usize) & (RING - 1);
        self.l[i] = x[0];
        self.l[i + RING] = x[0];
        self.r[i] = x[1];
        self.r[i + RING] = x[1];
        self.written += 1;
    }
    #[inline]
    fn get(&self, i: i64) -> [f32; 2] {
        if i < 0 || i as u64 >= self.written || self.written - i as u64 > RING as u64 {
            [0.0; 2]
        } else {
            let i = (i as usize) & (RING - 1);
            [self.l[i], self.r[i]]
        }
    }
    /// Band-limited read at fractional index `t` with bandwidth `s` (≤ 1,
    /// relative to this ring's Nyquist).
    #[inline]
    fn sinc(&self, k: &Kernel, t: f64, s: f32) -> [f32; 2] {
        let half = (ZC as f32 / s).ceil() as i64;
        let n0 = t.floor() as i64;
        let fr = (t - n0 as f64) as f32;
        let mut acc = [0.0f32; 2];
        for j in (n0 - half + 1)..=(n0 + half) {
            let d = (j - n0) as f32 - fr;
            let w = k.at((d * s).abs()) * s;
            if w != 0.0 {
                let x = self.get(j);
                acc[0] += x[0] * w;
                acc[1] += x[1] * w;
            }
        }
        acc
    }
    /// Same read through a polyphase table when it is ready and the window
    /// lies wholly inside what has been written; the direct form otherwise.
    #[inline]
    fn read(&self, k: &Kernel, ph: &mut Phases, t: f64, s: f32) -> [f32; 2] {
        if !ph.ready(k, s) {
            return self.sinc(k, t, s);
        }
        let half = ph.half as i64;
        let taps = 2 * ph.half;
        let n0 = t.floor() as i64;
        let first = n0 - half + 1;
        if first < 0 || (n0 + half) as u64 >= self.written || self.written - first as u64 > RING as u64 {
            return self.sinc(k, t, s);
        }
        let pf = (t - n0 as f64) as f32 * PHASES as f32;
        let p = (pf as usize).min(PHASES - 1);
        let f = pf - p as f32;
        let a = &ph.data[p * ph.stride..p * ph.stride + taps];
        let b = &ph.data[(p + 1) * ph.stride..(p + 1) * ph.stride + taps];
        let w = &mut ph.w[..taps];
        for ((w, a), b) in w.iter_mut().zip(a).zip(b) {
            *w = a + (b - a) * f;
        }
        let i0 = (first as usize) & (RING - 1);
        [dot(&self.l[i0..i0 + taps], w), dot(&self.r[i0..i0 + taps], w)]
    }
    #[inline]
    fn hold(&self, t: f64) -> [f32; 2] {
        self.get(t.floor() as i64)
    }
}

pub struct Clock {
    kernel: Kernel,
    fs: f64,
    input: Ring,
    output: Ring,
    ph_in: Phases,
    ph_out: Phases,
    /// Host-time of the next engine tick's input sample.
    next_in: f64,
    /// Host-time of the last produced engine tick.
    last_in: f64,
    /// Constant latency (host samples).
    delay: f64,
    /// Lookahead the input side keeps (host samples).
    in_guard: f64,
    host_n: u64,
}

impl Clock {
    pub fn new() -> Self {
        let mut c = Self {
            kernel: Kernel::new(),
            fs: 48_000.0,
            input: Ring::new(),
            output: Ring::new(),
            ph_in: Phases::new(),
            ph_out: Phases::new(),
            next_in: 0.0,
            last_in: 0.0,
            delay: 0.0,
            in_guard: 0.0,
            host_n: 0,
        };
        c.set_rate(48_000.0);
        c
    }

    /// Kernel half-width in host samples for the worst case (slowest clock).
    fn half_width_host(fs: f64) -> f64 {
        let slow = HW_RATE * SPEED_MIN;
        (ZC as f64 / CUTOFF) * (fs / slow).max(1.0)
    }

    pub fn set_rate(&mut self, fs: f64) {
        self.fs = fs;
        let w = Self::half_width_host(fs);
        self.in_guard = w.ceil() + 1.0;
        // Output side: half-width in engine samples converted to host time
        // at the slowest clock, plus one engine period of jitter.
        let slow = HW_RATE * SPEED_MIN;
        let w_out = (ZC as f64 / CUTOFF) * (fs / slow).max(1.0) + 2.0 * fs / slow;
        self.delay = (self.in_guard + w_out + 2.0).ceil();
        // Polyphase tables: room for the narrowest bandwidth either side
        // can ask for, built now for the nominal clock.
        let fast = HW_RATE * SPEED_MAX;
        self.ph_in.reserve(((slow / fs).min(1.0) * CUTOFF) as f32);
        self.ph_out.reserve(((fs / fast).min(1.0) * CUTOFF) as f32);
        self.ph_in.prebuild(&self.kernel, ((HW_RATE / fs).min(1.0) * CUTOFF) as f32);
        self.ph_out.prebuild(&self.kernel, ((fs / HW_RATE).min(1.0) * CUTOFF) as f32);
        self.reset();
    }

    pub fn reset(&mut self) {
        self.input.clear();
        self.output.clear();
        self.next_in = 0.0;
        self.last_in = 0.0;
        self.host_n = 0;
    }

    pub fn latency(&self) -> u32 {
        self.delay as u32
    }

    /// Push one host frame, run the engine for every tick that became due,
    /// and return one host frame of output.
    #[inline]
    pub fn process<F: FnMut([f32; 2]) -> [f32; 2]>(&mut self, x: [f32; 2], speed: f64, grit: bool, mut tick: F) -> [f32; 2] {
        self.input.push(x);
        let now = self.host_n as f64;
        self.host_n += 1;
        let rate = HW_RATE * speed.clamp(SPEED_MIN, SPEED_MAX);
        let period = self.fs / rate; // host samples per engine tick
        let s_in = ((rate / self.fs).min(1.0) * CUTOFF) as f32;

        while self.next_in + self.in_guard <= now {
            let xi = if grit { self.input.hold(self.next_in) } else { self.input.read(&self.kernel, &mut self.ph_in, self.next_in, s_in) };
            let y = tick(xi);
            self.output.push(y);
            self.last_in = self.next_in;
            self.next_in += period;
        }

        // Map output host-time to a fractional engine index.
        let t_out = now - self.delay;
        let j_last = self.output.written as f64 - 1.0;
        let u = j_last - (self.last_in - t_out) / period;
        if grit {
            self.output.hold(u)
        } else {
            let s_out = ((self.fs / rate).min(1.0) * CUTOFF) as f32;
            self.output.read(&self.kernel, &mut self.ph_out, u, s_out)
        }
    }
}

impl Default for Clock {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passes_a_tone_with_unity_gain_and_constant_delay() {
        for fs in [44_100.0, 48_000.0, 96_000.0] {
            let mut c = Clock::new();
            c.set_rate(fs);
            let f = 1000.0;
            let n = (fs * 0.5) as usize;
            let mut out = Vec::with_capacity(n);
            for i in 0..n {
                let x = (2.0 * std::f64::consts::PI * f * i as f64 / fs).sin() as f32 * 0.5;
                out.push(c.process([x, x], 1.0, false, |v| v)[0]);
            }
            let d = c.latency() as usize;
            let mut err = 0.0f32;
            for i in (n / 2)..n {
                let x = (2.0 * std::f64::consts::PI * f * (i - d) as f64 / fs).sin() as f32 * 0.5;
                err = err.max((out[i] - x).abs());
            }
            assert!(err < 0.02, "fs {fs}: err {err}, latency {d}");
        }
    }

    /// Moving DILATION switches to the direct form and back to a rebuilt
    /// table: both must agree, with no step where they hand over.
    #[test]
    fn table_and_direct_form_agree_through_a_speed_change() {
        let fs = 48_000.0;
        let run = |tables: bool| {
            let mut c = Clock::new();
            c.set_rate(fs);
            if !tables {
                // A table that can never be built: every read is direct.
                c.ph_in.stride = 0;
                c.ph_out.stride = 0;
                c.ph_in.rows = 0;
                c.ph_out.rows = 0;
            }
            (0..24_000)
                .map(|i| {
                    let x = (2.0 * std::f64::consts::PI * 700.0 * i as f64 / fs).sin() as f32 * 0.5;
                    let speed = if i < 8_000 { 1.0 } else if i < 16_000 { 0.61 } else { 1.7 };
                    c.process([x, -x], speed, false, |v| v)
                })
                .collect::<Vec<_>>()
        };
        let (a, b) = (run(true), run(false));
        let worst = a.iter().zip(&b).fold(0.0f32, |m, (p, q)| m.max((p[0] - q[0]).abs()).max((p[1] - q[1]).abs()));
        assert!(worst < 1e-4, "table vs direct: {worst}");
    }
}
