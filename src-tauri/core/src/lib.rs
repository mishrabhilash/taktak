//! TakTak core: everything latency-sensitive lives here, free of any UI dependency.
//!
//! Event flow: `input` (OS hook thread) → `Trigger` over a lock-free SPSC ring →
//! `audio` (real-time callback) → speakers. See `docs/architecture.md`.
//!
//! Privacy: key identities exist only in memory, only long enough to choose a sound.
//! [`key::Key`]'s `Debug` impl is deliberately redacted so a stray `{:?}` can never log one.

pub mod audio;
pub mod clock;
pub mod input;
pub mod key;
pub mod latency;
pub mod pack;
pub mod synth;
