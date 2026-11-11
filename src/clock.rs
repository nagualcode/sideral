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

/// Stereo ring addressed by absolute sample index.
struct Ring {
    buf: Vec<[f32; 2]>,
    written: u64,
}

impl Ring {
    fn new() -> Self {
        Self { buf: vec![[0.0; 2]; RING], written: 0 }
    }
    fn clear(&mut self) {
        self.buf.fill([0.0; 2]);
        self.written = 0;
    }
    #[inline]
    fn push(&mut self, x: [f32; 2]) {
        self.buf[(self.written as usize) & (RING - 1)] = x;
        self.written += 1;
    }
    #[inline]
    fn get(&self, i: i64) -> [f32; 2] {
        if i < 0 || i as u64 >= self.written || self.written - i as u64 > RING as u64 {
            [0.0; 2]
        } else {
            self.buf[(i as usize) & (RING - 1)]
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
            let xi = if grit { self.input.hold(self.next_in) } else { self.input.sinc(&self.kernel, self.next_in, s_in) };
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
            self.output.sinc(&self.kernel, u, s_out)
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
}
