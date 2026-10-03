//! Monotonic nanosecond clock shared by the input hook, the audio callback and latency stats.
//!
//! On macOS this is `mach_absolute_time` converted to ns, the same timebase CoreAudio host
//! timestamps and CGEvent timestamps use, so values from all three are directly comparable.

#[cfg(target_os = "macos")]
mod imp {
    use std::sync::OnceLock;

    #[repr(C)]
    struct MachTimebaseInfo {
        numer: u32,
        denom: u32,
    }

    unsafe extern "C" {
        fn mach_absolute_time() -> u64;
        fn mach_timebase_info(info: *mut MachTimebaseInfo) -> i32;
    }

    fn timebase() -> (u64, u64) {
        static TB: OnceLock<(u64, u64)> = OnceLock::new();
        *TB.get_or_init(|| {
            let mut info = MachTimebaseInfo { numer: 0, denom: 0 };
            unsafe { mach_timebase_info(&mut info) };
            (info.numer as u64, info.denom.max(1) as u64)
        })
    }

    pub fn ticks() -> u64 {
        unsafe { mach_absolute_time() }
    }

    pub fn ticks_to_ns(ticks: u64) -> u64 {
        let (numer, denom) = timebase();
        if numer == denom {
            ticks
        } else {
            ((ticks as u128 * numer as u128) / denom as u128) as u64
        }
    }

    pub fn now_ns() -> u64 {
        ticks_to_ns(ticks())
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    use std::sync::OnceLock;
    use std::time::Instant;

    pub fn now_ns() -> u64 {
        static EPOCH: OnceLock<Instant> = OnceLock::new();
        EPOCH.get_or_init(Instant::now).elapsed().as_nanos() as u64
    }
}

pub use imp::*;
