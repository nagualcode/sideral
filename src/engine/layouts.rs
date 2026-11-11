//! Memory layouts recovered from the COSMOS firmware.
//!
//! Every number in this file was read out of the decompiled ARM Cortex-M4
//! images (`COSMOS_M1.41.smf` for the main set, `COSMOS_Q1.41.smf` for the
//! quadro set). The hardware keeps one 8 MB SDRAM bank at `0xC0000000`
//! holding exactly 2^21 floats; each algorithm lays its delay lines out back
//! to back from index 0 and leaves the remainder as a "tail" that keeps a
//! running recording of the newest material. Switching algorithms does not
//! copy anything: the new algorithm simply reinterprets the same memory with
//! its own segmentation. That is the whole "memory transfer" mechanism.
//!
//! Lengths are in samples at the hardware rate of 46 875 Hz (the codec is
//! clocked by `HAL_I2S_Init(AudioFreq = 0xB71B)`; 2^21 / 46 875 = 44.739 s,
//! the "44.7 s" printed in the manual).

/// Total sound memory: 2^21 floats, as on the 8 MB SDRAM.
pub const MEM: usize = 1 << 21;
/// Hardware sample rate (I2S + TIM2 at 90 MHz / 1920).
pub const HW_RATE: f64 = 46_875.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Family {
    TwoDelays,
    FourDelays,
    Reverb,
    Granular,
    /// The rhythmic firmware's loopers (two 2^20 halves, one per channel).
    Pulsar,
}

/// Granular geometry: buffer length, the triangular grain and its quarter
/// offsets, plus the optional "central" granular delay of the large size.
#[derive(Clone, Copy, Debug)]
pub struct Grains {
    /// Grain length (the counter wraps at `len - 1`).
    pub len: i32,
    /// First index of the falling half of the triangle window.
    pub bound: i32,
    /// `len - 1`: the falling half is `end - i`.
    pub end: i32,
    /// Window peak divisor.
    pub half: f32,
    /// Phase offsets of grains 2, 3, 4 (grain 1 sits at 0).
    pub offs: [i32; 3],
}

#[derive(Clone, Copy, Debug)]
pub struct Layout {
    pub family: Family,
    /// Segment lengths, laid out from index 0 in this order.
    /// 2D: [A, B]; 4D: [A, B, C, D]; reverb: [AP1, D1, AP2, D2, AP3, D3, AP4, D4];
    /// granular: [buffer].
    pub lines: &'static [usize],
    /// Base index used for the extra 120-sample "V" notch the firmware cuts
    /// at `base + tail_pointer` when leaving this layout (`None` = no notch).
    /// The 4-delay layouts put it at the start of line C rather than at the
    /// tail, and the small granular one four samples early: both are quirks
    /// of the original code, kept on purpose.
    pub tail_notch: Option<usize>,
    /// When true the tail is written with zeros (4D large) instead of audio.
    pub tail_zero: bool,
    /// Granular grains (main, per channel) and the central grains.
    pub grains: Option<Grains>,
    pub center: Option<Grains>,
    /// Output gain applied to the grain sum (0.7 large, 0.8 others).
    pub grain_gain: f32,
}

impl Layout {
    pub fn tail_base(&self) -> usize {
        self.lines.iter().sum()
    }
    pub fn tail_len(&self) -> usize {
        MEM - self.tail_base()
    }
    pub fn base(&self, k: usize) -> usize {
        self.lines[..k].iter().sum()
    }
}

const fn grains(len: i32, bound: i32, half: f32, offs: [i32; 3]) -> Grains {
    Grains { len, bound, end: len - 1, half, offs }
}

const fn delays(lines: &'static [usize], notch: Option<usize>, zero: bool) -> Layout {
    let family = if lines.len() == 2 { Family::TwoDelays } else { Family::FourDelays };
    Layout { family, lines, tail_notch: notch, tail_zero: zero, grains: None, center: None, grain_gain: 0.0 }
}

const fn reverb(lines: &'static [usize]) -> Layout {
    Layout { family: Family::Reverb, lines, tail_notch: None, tail_zero: false, grains: None, center: None, grain_gain: 0.0 }
}

const fn granular(lines: &'static [usize], notch: usize, g: Grains, center: Option<Grains>, gain: f32) -> Layout {
    Layout {
        family: Family::Granular,
        lines,
        tail_notch: Some(notch),
        tail_zero: false,
        grains: Some(g),
        center,
        grain_gain: gain,
    }
}

// ---------------------------------------------------------------------------
// Main firmware (COSMOS_M1.41) — panel order: 2D S/M/L, 4D S/M/L,
// reverb S/M/L, granular S/M/L. (The rotary switch is one-hot: position 1
// is bit 0x800 and position 12 is bit 0x001.)
// ---------------------------------------------------------------------------

const M_2DS: &[usize] = &[100_003, 123_113];
const M_2DM: &[usize] = &[468_761, 431_251];
const M_2DL: &[usize] = &[1_000_003, 1_097_101];
const M_4DS: &[usize] = &[133_013, 152_003, 88_001, 111_029];
/// Line A wraps on `332837 < i`, so it is 332 838 long: the one non-prime
/// delay of the main set (an off-by-one in the firmware, kept).
const M_4DM: &[usize] = &[332_838, 234_383, 604_711, 454_709];
const M_4DL: &[usize] = &[576_577, 426_563, 328_127, 764_081];
const M_RS: &[usize] = &[43_156, 59_661, 89_175, 65_182, 100_559, 64_189, 97_482, 68_677];
const M_RM: &[usize] = &[201_119, 119_321, 178_349, 130_363, 86_311, 128_377, 194_963, 137_353];
const M_RL: &[usize] = &[341_911, 202_859, 146_743, 221_621, 303_217, 218_249, 331_447, 233_509];
const M_GS: &[usize] = &[75_003];
const M_GM: &[usize] = &[234_376];
const M_GL: &[usize] = &[334_376];

pub static MAIN: [Layout; 12] = [
    delays(M_2DS, Some(223_116), false),
    delays(M_2DM, Some(900_012), false),
    delays(M_2DL, Some(2_097_104), false),
    delays(M_4DS, Some(285_016), false),
    delays(M_4DM, Some(567_221), false),
    delays(M_4DL, None, true),
    reverb(M_RS),
    reverb(M_RM),
    reverb(M_RL),
    granular(M_GS, 74_999, grains(9_361, 4_681, 4_680.0, [2_340, 4_680, 7_020]), None, 0.8),
    granular(M_GM, 234_376, grains(20_361, 10_181, 10_180.0, [5_090, 10_180, 15_270]), None, 0.8),
    granular(
        M_GL,
        334_376,
        grains(27_361, 13_681, 13_680.0, [6_840, 13_680, 20_520]),
        Some(Grains { len: 12_456, bound: 6_228, end: 12_455, half: 6_227.0, offs: [3_113, 6_227, 9_339] }),
        0.7,
    ),
];

// ---------------------------------------------------------------------------
// Quadro firmware (COSMOS_Q1.41): the counterpart set of primes meant to run
// on a second unit for the rear pair. Same algorithms, other constellation.
// ---------------------------------------------------------------------------

const Q_2DS: &[usize] = &[110_119, 130_121];
const Q_2DM: &[usize] = &[498_761, 538_763];
const Q_2DL: &[usize] = &[820_037, 920_011];
const Q_4DS: &[usize] = &[143_063, 112_019, 60_013, 92_003];
const Q_4DM: &[usize] = &[302_843, 224_423, 504_727, 354_727];
const Q_4DL: &[usize] = &[526_583, 376_573, 288_137, 804_091];
const Q_RS: &[usize] = &[87_180, 61_667, 42_157, 67_187, 80_567, 66_191, 107_507, 69_678];
const Q_RM: &[usize] = &[231_131, 159_337, 89_317, 170_363, 208_367, 126_397, 224_969, 167_381];
const Q_RL: &[usize] = &[301_913, 232_861, 116_747, 181_639, 263_227, 258_253, 311_447, 193_513];
const Q_GS: &[usize] = &[65_003];
const Q_GM: &[usize] = &[274_376];
const Q_GL: &[usize] = &[374_376];

pub static QUAD: [Layout; 12] = [
    delays(Q_2DS, Some(240_240), false),
    delays(Q_2DM, Some(1_037_524), false),
    delays(Q_2DL, Some(1_740_048), false),
    delays(Q_4DS, Some(255_082), false),
    delays(Q_4DM, Some(527_266), false),
    delays(Q_4DL, None, true),
    reverb(Q_RS),
    reverb(Q_RM),
    reverb(Q_RL),
    granular(Q_GS, 65_003, grains(8_361, 4_181, 4_180.0, [2_090, 4_180, 6_270]), None, 0.8),
    granular(Q_GM, 274_376, grains(17_361, 8_681, 8_680.0, [4_340, 8_680, 13_020]), None, 0.8),
    granular(
        Q_GL,
        374_376,
        grains(25_361, 12_681, 12_680.0, [6_340, 12_680, 19_020]),
        Some(Grains { len: 16_456, bound: 8_228, end: 16_455, half: 8_227.0, offs: [4_113, 8_227, 12_339] }),
        0.7,
    ),
];

pub static PULSAR: Layout = Layout {
    family: Family::Pulsar,
    lines: &[1_048_575, 1_048_575],
    tail_notch: None,
    tail_zero: false,
    grains: None,
    center: None,
    grain_gain: 0.0,
};

/// Names of the 21 programs.
pub const NAMES: [&str; 21] = [
    "Binary · I",
    "Binary · II",
    "Binary · III",
    "Trapezium · I",
    "Trapezium · II",
    "Trapezium · III",
    "Nebula · I",
    "Nebula · II",
    "Nebula · III",
    "Stardust · I",
    "Stardust · II",
    "Stardust · III",
    "Pulsar",
    "Resonance 8:7",
    "Resonance 4:3",
    "Roche ÷8",
    "Roche ÷6",
    "Roche ÷5",
    "Supernova ÷8",
    "Supernova ÷6",
    "Kepler",
];

#[cfg(test)]
mod tests {
    use super::*;

    fn is_prime(n: usize) -> bool {
        if n < 2 {
            return false;
        }
        let mut i = 2;
        while i * i <= n {
            if n % i == 0 {
                return false;
            }
            i += 1;
        }
        true
    }

    #[test]
    fn layouts_fit_memory() {
        for set in [&MAIN, &QUAD] {
            for l in set.iter() {
                assert!(l.tail_base() <= MEM, "{:?}", l.lines);
            }
        }
    }

    #[test]
    fn delay_lines_are_prime() {
        for (set, name) in [(&MAIN, "main"), (&QUAD, "quad")] {
            for (i, l) in set.iter().enumerate().take(6) {
                for &n in l.lines {
                    // The single documented exception: main 4D-M line A.
                    if name == "main" && i == 4 && n == 332_838 {
                        continue;
                    }
                    assert!(is_prime(n), "{name} alg {i}: {n} not prime");
                }
            }
        }
    }
}
