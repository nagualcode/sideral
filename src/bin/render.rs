//! Offline renderer: runs the engine (through the same host clock as the
//! plugin) over a WAV file or a built-in test phrase and writes the result.
//!
//! ```sh
//! cargo run --release --bin sideral-render -- out.wav [in.wav] [key=value ...]
//! ```
//! Keys: program (0-20), quad (0/1), blur, drift, drive, supcom, fb, mix,
//! seconds, rate, clock, grit (0/1), hpf, lpf, record (0-3), reverse (0/1).

use sideral::clock::Clock;
use sideral::engine::{Controls, Engine};
use std::io::{Read, Write};

fn read_wav(path: &str) -> (u32, Vec<[f32; 2]>) {
    let mut f = std::fs::File::open(path).expect("open input");
    let mut b = Vec::new();
    f.read_to_end(&mut b).expect("read input");
    let u16le = |i: usize| u16::from_le_bytes([b[i], b[i + 1]]);
    let u32le = |i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
    let mut i = 12;
    let (mut ch, mut rate, mut bits, mut fmt) = (2u16, 48_000u32, 16u16, 1u16);
    let mut data = (0, 0);
    while i + 8 <= b.len() {
        let id = &b[i..i + 4];
        let len = u32le(i + 4) as usize;
        if id == b"fmt " {
            fmt = u16le(i + 8);
            ch = u16le(i + 10);
            rate = u32le(i + 12);
            bits = u16le(i + 22);
        } else if id == b"data" {
            data = (i + 8, len.min(b.len() - i - 8));
        }
        i += 8 + len + (len & 1);
    }
    let bps = (bits / 8) as usize;
    let frame = bps * ch as usize;
    let n = data.1 / frame;
    let sample = |o: usize| -> f32 {
        match (fmt, bits) {
            (3, 32) => f32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]),
            (_, 16) => i16::from_le_bytes([b[o], b[o + 1]]) as f32 / 32768.0,
            (_, 24) => ((i32::from(b[o]) << 8 | i32::from(b[o + 1]) << 16 | i32::from(b[o + 2]) << 24) >> 8) as f32 / 8_388_608.0,
            (_, 32) => i32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]) as f32 / 2_147_483_648.0,
            _ => 0.0,
        }
    };
    let out = (0..n)
        .map(|k| {
            let o = data.0 + k * frame;
            let l = sample(o);
            let r = if ch > 1 { sample(o + bps) } else { l };
            [l, r]
        })
        .collect();
    (rate, out)
}

fn write_wav(path: &str, rate: u32, x: &[[f32; 2]]) {
    let mut f = std::fs::File::create(path).expect("create output");
    let data = (x.len() * 8) as u32;
    let mut h = Vec::new();
    h.extend_from_slice(b"RIFF");
    h.extend_from_slice(&(36 + data).to_le_bytes());
    h.extend_from_slice(b"WAVEfmt ");
    h.extend_from_slice(&16u32.to_le_bytes());
    h.extend_from_slice(&3u16.to_le_bytes());
    h.extend_from_slice(&2u16.to_le_bytes());
    h.extend_from_slice(&rate.to_le_bytes());
    h.extend_from_slice(&(rate * 8).to_le_bytes());
    h.extend_from_slice(&8u16.to_le_bytes());
    h.extend_from_slice(&32u16.to_le_bytes());
    h.extend_from_slice(b"data");
    h.extend_from_slice(&data.to_le_bytes());
    f.write_all(&h).unwrap();
    let mut body = Vec::with_capacity(x.len() * 8);
    for s in x {
        body.extend_from_slice(&s[0].to_le_bytes());
        body.extend_from_slice(&s[1].to_le_bytes());
    }
    f.write_all(&body).unwrap();
}

/// A short phrase of plucked partials, then silence: lets the memory speak.
fn phrase(rate: u32, seconds: f32) -> Vec<[f32; 2]> {
    let n = (rate as f32 * seconds) as usize;
    let notes = [(0.0, 220.0), (0.7, 277.18), (1.3, 329.63), (2.1, 415.3), (2.9, 246.94), (3.6, 369.99)];
    (0..n)
        .map(|i| {
            let t = i as f32 / rate as f32;
            let mut s = 0.0;
            for &(t0, f) in &notes {
                let dt = t - t0;
                if (0.0..4.0).contains(&dt) {
                    let env = (-dt * 1.6).exp() * (1.0 - (-dt * 400.0).exp());
                    for h in 1..5 {
                        s += env * (std::f32::consts::TAU * f * h as f32 * dt).sin() / (h * h) as f32;
                    }
                }
            }
            [s * 0.3, s * 0.3]
        })
        .collect()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let out = args.first().cloned().unwrap_or_else(|| "sideral.wav".into());
    let mut c = Controls { gate: 0, ..Controls::default() };
    let (mut seconds, mut rate, mut speed, mut grit) = (40.0f32, 48_000u32, 1.0f64, false);
    let mut input: Option<String> = None;
    for a in &args[1.min(args.len())..] {
        if let Some((k, v)) = a.split_once('=') {
            let f: f32 = v.parse().unwrap_or(0.0);
            match k {
                "program" => c.algorithm = f as usize,
                "quad" => c.quad = f > 0.5,
                "blur" => c.blur = f,
                "drift" => c.drift = f,
                "drive" => c.drive = f,
                "supcom" => c.supcom = f,
                "fb" => c.fb = f,
                "mix" => c.mix = f,
                "seconds" => seconds = f,
                "rate" => rate = f as u32,
                "clock" => speed = f64::from(f),
                "grit" => grit = f > 0.5,
                "hpf" => c.hpf = f as u8,
                "lpf" => c.lpf = f as u8,
                "record" => c.record = f as u8,
                "reverse" => c.reverse = f > 0.5,
                "half" => c.half_speed = f > 0.5,
                _ => eprintln!("unknown key {k}"),
            }
        } else {
            input = Some(a.clone());
        }
    }
    let (rate, src) = match &input {
        Some(p) => read_wav(p),
        None => (rate, phrase(rate, 5.0)),
    };
    let n = ((seconds * rate as f32) as usize).max(src.len());
    let mut engine = Engine::new();
    engine.prime(&c);
    let mut clock = Clock::new();
    clock.set_rate(f64::from(rate));
    let t0 = std::time::Instant::now();
    let mut y = Vec::with_capacity(n);
    for i in 0..n {
        let x = src.get(i).copied().unwrap_or([0.0; 2]);
        y.push(clock.process(x, speed, grit, |v| {
            let (l, r) = engine.tick(&c, v[0], v[1]);
            [l, r]
        }));
    }
    let dt = t0.elapsed().as_secs_f64();
    write_wav(&out, rate, &y);
    let peak = y.iter().fold(0.0f32, |m, s| m.max(s[0].abs()).max(s[1].abs()));
    println!(
        "{out}: {n} frames @ {rate} Hz, peak {peak:.3}, latency {} samples, {:.1}x realtime",
        clock.latency(),
        n as f64 / f64::from(rate) / dt
    );
}
