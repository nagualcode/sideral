//! Lock-free bridge from the audio thread to the editor: a coarse picture
//! of the whole sound memory plus the engine's telemetry.

use std::sync::atomic::{AtomicU32, Ordering::Relaxed};

/// Bins of the memory envelope (2^21 / 512 = 4096 samples per bin).
pub const BINS: usize = 512;
const SLOTS: usize = 64;

pub struct Scope {
    env: Box<[AtomicU32]>,
    slots: Box<[AtomicU32]>,
    /// Incremented by the editor's ERASE ALL button.
    pub erase_all: AtomicU32,
}

impl Default for Scope {
    fn default() -> Self {
        Self {
            env: (0..BINS).map(|_| AtomicU32::new(0)).collect(),
            slots: (0..SLOTS).map(|_| AtomicU32::new(0)).collect(),
            erase_all: AtomicU32::new(0),
        }
    }
}

/// Named telemetry slots.
#[derive(Clone, Copy)]
#[repr(usize)]
pub enum Slot {
    Algorithm = 0,
    Quad,
    MemoryPeak,
    InputPeak,
    OutputPeak,
    Suppress,
    Limiter,
    Switching,
    Settle,
    Clearing,
    Reverse,
    Lfo0,
    Lfo1,
    Lfo2,
    Lfo3,
    Chaos0,
    Chaos1,
    Speed,
    Head0 = 18, // .. Head8 (9 slots)
    Grain0 = 27, // .. Grain11 (12 slots)
    LoopState = 39,
    LoopLen,
    Slices,
    SliceSrcL,
    SliceSrcR,
    SliceQ,
    Flash,
    Eclipse,
    Redshift,
}

impl Scope {
    #[inline]
    pub fn set(&self, s: usize, v: f32) {
        if let Some(a) = self.slots.get(s) {
            a.store(v.to_bits(), Relaxed);
        }
    }
    #[inline]
    pub fn get(&self, s: usize) -> f32 {
        self.slots.get(s).map_or(0.0, |a| f32::from_bits(a.load(Relaxed)))
    }
    #[inline]
    pub fn put(&self, s: Slot, v: f32) {
        self.set(s as usize, v);
    }
    #[inline]
    pub fn read(&self, s: Slot) -> f32 {
        self.get(s as usize)
    }
    #[inline]
    pub fn set_bin(&self, i: usize, v: f32) {
        if let Some(a) = self.env.get(i) {
            a.store(v.to_bits(), Relaxed);
        }
    }
    #[inline]
    pub fn bin(&self, i: usize) -> f32 {
        self.env.get(i).map_or(0.0, |a| f32::from_bits(a.load(Relaxed)))
    }
}
