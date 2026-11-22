//! libm calls that must reach glibc exactly as the C code makes them.
//!
//! LLVM rewrites `pow(2.0, x)` into `exp2(x)` and `pow(10.0, x)` into
//! `exp10(x)` even without fast-math (LibCallSimplifier); GCC does not
//! (only under `-funsafe-math-optimizations`). glibc's `exp2`/`exp10` are
//! not guaranteed to round like its `pow`, so the rewrite could change the
//! last bit of the chip's filter cutoff and with it the PCM. Hiding the
//! constant base behind `black_box` keeps the call a real `pow`.

/// `pow(x, y)` from the platform libm (glibc on Linux), never rewritten.
#[inline(always)]
pub fn pow(x: f64, y: f64) -> f64 {
    std::hint::black_box(x).powf(y)
}
