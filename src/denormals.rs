//! Subnormal protection for the audio thread.

/// Flush-to-zero / denormals-are-zero for the audio thread, restored on
/// drop. A memory fading out, the button and knob smoothers and the
/// envelopes all decay into the subnormal range; on x86 every operation on
/// such a value traps to microcode (a hundred times slower), which shows
/// up as CPU load creeping in once the input has been quiet for a while.
pub struct NoDenormals(u64);

impl NoDenormals {
    #[inline]
    pub fn new() -> Self {
        #[cfg(target_arch = "x86_64")]
        {
            let mut old: u32 = 0;
            // SAFETY: MXCSR read/write; SSE is baseline on x86-64.
            unsafe {
                core::arch::asm!("stmxcsr [{p}]", p = in(reg) &mut old, options(nostack, preserves_flags));
                let new = old | 0x8040; // FTZ | DAZ
                core::arch::asm!("ldmxcsr [{p}]", p = in(reg) &new, options(nostack, preserves_flags, readonly));
            }
            Self(u64::from(old))
        }
        #[cfg(target_arch = "aarch64")]
        {
            let old: u64;
            // SAFETY: FPCR is a user-accessible register; bit 24 is FZ.
            unsafe {
                core::arch::asm!("mrs {o}, fpcr", o = out(reg) old, options(nomem, nostack, preserves_flags));
                core::arch::asm!("msr fpcr, {n}", n = in(reg) old | (1 << 24), options(nomem, nostack, preserves_flags));
            }
            Self(old)
        }
        #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
        {
            Self(0)
        }
    }
}

impl Default for NoDenormals {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for NoDenormals {
    #[inline]
    fn drop(&mut self) {
        #[cfg(target_arch = "x86_64")]
        {
            let old = self.0 as u32;
            // SAFETY: restores the MXCSR value read in `new`.
            unsafe {
                core::arch::asm!("ldmxcsr [{p}]", p = in(reg) &old, options(nostack, preserves_flags, readonly));
            }
        }
        #[cfg(target_arch = "aarch64")]
        {
            // SAFETY: restores the FPCR value read in `new`.
            unsafe {
                core::arch::asm!("msr fpcr, {n}", n = in(reg) self.0, options(nomem, nostack, preserves_flags));
            }
        }
    }
}
