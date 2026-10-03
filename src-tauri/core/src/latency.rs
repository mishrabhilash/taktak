//! Keypress-to-playback latency measurement. Records timings only — never key identities.
//!
//! A keystroke's path is split into three measured stages:
//! - `input`:  OS event timestamp → our hook callback (OS input pipeline)
//! - `queue`:  hook callback → audio callback picks the trigger up (≤ one buffer period)
//! - `output`: audio callback → sound leaves the device, as estimated by the audio backend
//!   (buffer + device latency + safety offset on CoreAudio)

use std::fmt;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LatencySample {
    pub input_ns: u64,
    pub queue_ns: u64,
    pub output_ns: u64,
}

impl LatencySample {
    pub fn total_ns(&self) -> u64 {
        self.input_ns + self.queue_ns + self.output_ns
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Percentiles {
    pub p50: f64,
    pub p95: f64,
    pub max: f64,
}

impl Percentiles {
    /// From values in milliseconds; nearest-rank percentiles.
    pub fn from_ms(values: &mut [f64]) -> Percentiles {
        if values.is_empty() {
            return Percentiles::default();
        }
        values.sort_by(|a, b| a.total_cmp(b));
        let rank =
            |p: f64| values[((p * values.len() as f64).ceil() as usize).clamp(1, values.len()) - 1];
        Percentiles { p50: rank(0.50), p95: rank(0.95), max: values[values.len() - 1] }
    }
}

impl fmt::Display for Percentiles {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "p50 {:5.2}  p95 {:5.2}  max {:5.2} ms", self.p50, self.p95, self.max)
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Report {
    pub count: usize,
    pub total: Percentiles,
    pub input: Percentiles,
    pub queue: Percentiles,
    pub output: Percentiles,
}

impl Report {
    pub fn from_samples(samples: &[LatencySample]) -> Report {
        let col = |f: &dyn Fn(&LatencySample) -> u64| {
            let mut v: Vec<f64> = samples.iter().map(|s| f(s) as f64 / 1e6).collect();
            Percentiles::from_ms(&mut v)
        };
        Report {
            count: samples.len(),
            total: col(&|s| s.total_ns()),
            input: col(&|s| s.input_ns),
            queue: col(&|s| s.queue_ns),
            output: col(&|s| s.output_ns),
        }
    }
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "keypress→sound latency over {} presses", self.count)?;
        writeln!(f, "  total   {}", self.total)?;
        writeln!(f, "  input   {}   (OS event → hook)", self.input)?;
        writeln!(f, "  queue   {}   (hook → audio callback)", self.queue)?;
        write!(f, "  output  {}   (audio callback → speaker, backend estimate)", self.output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentiles_nearest_rank() {
        let mut v: Vec<f64> = (1..=100).map(f64::from).collect();
        let p = Percentiles::from_ms(&mut v);
        assert_eq!((p.p50, p.p95, p.max), (50.0, 95.0, 100.0));
        assert_eq!(Percentiles::from_ms(&mut []), Percentiles::default());
        assert_eq!(Percentiles::from_ms(&mut [3.0]).p95, 3.0);
    }

    #[test]
    fn report_sums_stages() {
        let s = LatencySample { input_ns: 1_000_000, queue_ns: 2_000_000, output_ns: 3_000_000 };
        let r = Report::from_samples(&[s, s]);
        assert_eq!(r.count, 2);
        assert_eq!(r.total.p50, 6.0);
        assert_eq!(r.queue.max, 2.0);
    }
}
