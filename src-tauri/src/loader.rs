//! The pack loader thread (`taktak-loader`): decodes packs at the engine's rate off every
//! thread that matters (main, control, audio, input), and hands the finished banks and
//! preview clips back to the control thread.
//!
//! Requests that pile up while a load runs are coalesced: only the newest bank request and
//! the newest preview request are served, so clicking through packs quickly never queues a
//! backlog of decodes.

use std::path::Path;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use taktak_core::audio::SoundBank;
use taktak_core::pack::{self, LoadedPack, PackError, PackInfo};
use taktak_core::synth;

/// Load the first pack of `candidates` that works, at `rate`.
pub struct BankJob {
    /// Echoed back, so the control thread can drop results it no longer wants.
    pub generation: u64,
    pub rate: u32,
    pub candidates: Vec<PackInfo>,
}

/// Decode `info`'s preview clip at `rate`.
pub struct PreviewJob {
    pub info: PackInfo,
    pub rate: u32,
}

/// A finished [`BankJob`].
pub struct BankLoaded {
    pub generation: u64,
    pub rate: u32,
    /// The pack the bank comes from; `None` = the built-in click.
    pub playing: Option<PackInfo>,
    pub bank: SoundBank,
    pub preview: Option<Box<[f32]>>,
    /// The candidates tried before `playing`, with why each failed.
    pub failures: Vec<(PackInfo, PackError)>,
    pub elapsed: Duration,
}

/// A finished [`PreviewJob`]. `clip` is `None` when the pack has no preview sound or failed to
/// load; `error` says why it failed (`None`: it has no preview sound).
pub struct PreviewLoaded {
    pub id: String,
    pub rate: u32,
    pub clip: Option<Box<[f32]>>,
    pub error: Option<PackError>,
}

/// What the loader hands back. (A bank result is boxed: it is far larger than the rest.)
pub enum Done {
    Bank(Box<BankLoaded>),
    Preview(PreviewLoaded),
}

enum Job {
    Bank(BankJob),
    Preview(PreviewJob),
}

/// The built-in click: synthesized press and release sounds for every key, plus its press as
/// the preview.
pub fn builtin_bank(rate: u32) -> (SoundBank, Box<[f32]>) {
    let press = synth::click(&synth::ClickParams::PRESS, rate, 0.8);
    let release = synth::click(&synth::ClickParams::RELEASE, rate, 0.5);
    (SoundBank::uniform(press.clone(), Some(release)), press)
}

/// What [`load_first`] settled on.
pub struct Picked {
    /// The pack that loaded; `None` = the built-in click.
    pub playing: Option<PackInfo>,
    pub bank: SoundBank,
    pub preview: Option<Box<[f32]>>,
    /// The candidates tried before it, with why each failed.
    pub failures: Vec<(PackInfo, PackError)>,
}

/// Tries `candidates` in order with `load` and returns the first that works, or the built-in
/// click, plus the failures before it.
pub fn load_first(
    candidates: &[PackInfo],
    rate: u32,
    mut load: impl FnMut(&PackInfo, u32) -> Result<LoadedPack, PackError>,
) -> Picked {
    let mut failures = Vec::new();
    for info in candidates {
        match load(info, rate) {
            Ok(loaded) => {
                return Picked {
                    playing: Some(info.clone()),
                    bank: loaded.bank,
                    preview: loaded.preview,
                    failures,
                };
            }
            Err(err) => failures.push((info.clone(), err)),
        }
    }
    let (bank, preview) = builtin_bank(rate);
    Picked { playing: None, bank, preview: Some(preview), failures }
}

/// Decodes a pack from disk (`pack::load`).
pub fn load_pack(info: &PackInfo, rate: u32) -> Result<LoadedPack, PackError> {
    pack::load(Path::new(&info.location), info.origin, rate)
}

/// Handle to the loader thread; dropping it lets the thread finish its current job and exit.
pub struct Loader {
    jobs: Option<Sender<Job>>,
    thread: Option<JoinHandle<()>>,
}

impl Loader {
    /// Starts the thread. `done` receives every result, on the loader thread.
    pub fn spawn(done: impl Fn(Done) + Send + 'static) -> std::io::Result<Loader> {
        let (tx, rx) = mpsc::channel();
        let thread = thread::Builder::new()
            .name("taktak-loader".into())
            .spawn(move || serve(rx, done, load_pack))?;
        Ok(Loader { jobs: Some(tx), thread: Some(thread) })
    }

    pub fn load_bank(&self, job: BankJob) {
        self.send(Job::Bank(job));
    }

    pub fn load_preview(&self, job: PreviewJob) {
        self.send(Job::Preview(job));
    }

    fn send(&self, job: Job) {
        if let Some(jobs) = &self.jobs
            && jobs.send(job).is_err()
        {
            log::warn!("the pack loader has stopped");
        }
    }
}

impl Drop for Loader {
    fn drop(&mut self) {
        self.jobs = None;
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The loader thread's loop. Blocks while idle.
fn serve(
    rx: Receiver<Job>,
    done: impl Fn(Done),
    load: impl Fn(&PackInfo, u32) -> Result<LoadedPack, PackError>,
) {
    while let Ok(first) = rx.recv() {
        let (mut bank, mut preview) = (None, None);
        for job in std::iter::once(first).chain(rx.try_iter()) {
            match job {
                Job::Bank(job) => bank = Some(job),
                Job::Preview(job) => preview = Some(job),
            }
        }
        if let Some(job) = bank {
            let started = Instant::now();
            let Picked { playing, bank, preview, failures } =
                load_first(&job.candidates, job.rate, |info, rate| load(info, rate));
            done(Done::Bank(Box::new(BankLoaded {
                generation: job.generation,
                rate: job.rate,
                playing,
                bank,
                preview,
                failures,
                elapsed: started.elapsed(),
            })));
        }
        if let Some(job) = preview {
            let (clip, error) = match load(&job.info, job.rate) {
                Ok(loaded) => (loaded.preview, None),
                Err(err) => {
                    log::warn!("preview not loaded: {err}");
                    (None, Some(err))
                }
            };
            done(Done::Preview(PreviewLoaded { id: job.info.id, rate: job.rate, clip, error }));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use taktak_core::audio::Variation;
    use taktak_core::pack::{PackOrigin, Problem};

    fn info(id: &str) -> PackInfo {
        PackInfo {
            id: id.into(),
            name: id.to_uppercase(),
            version: None,
            author: "A".into(),
            license: "CC0-1.0".into(),
            description: None,
            source: None,
            attribution: None,
            location: PathBuf::from(format!("/packs/{id}")),
            origin: PackOrigin::Bundled,
        }
    }

    /// Packs whose id starts with "bad" fail; others load a one-sample bank at `rate` whose
    /// value is the id's length.
    fn fake_load(info: &PackInfo, rate: u32) -> Result<LoadedPack, PackError> {
        if info.id.starts_with("bad") {
            return Err(PackError::single(&info.location, Problem::error("pack.json", "broken")));
        }
        let clip = vec![info.id.len() as f32; rate as usize / 1000].into_boxed_slice();
        Ok(LoadedPack {
            info: info.clone(),
            bank: SoundBank::uniform(clip.clone(), None),
            preview: Some(clip),
            warnings: vec![],
        })
    }

    #[test]
    fn first_working_candidate_wins() {
        let Picked { playing, bank, preview, failures } =
            load_first(&[info("bad-1"), info("good"), info("other")], 48_000, fake_load);
        assert_eq!(playing.unwrap().id, "good");
        assert_eq!(bank.samples[0][0], 4.0);
        assert_eq!(preview.unwrap().len(), 48);
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].0.id, "bad-1");
    }

    #[test]
    fn all_failing_falls_back_to_the_built_in_click() {
        let Picked { playing, bank, preview, failures } =
            load_first(&[info("bad-1"), info("bad-2")], 44_100, fake_load);
        assert!(playing.is_none());
        assert_eq!(failures.len(), 2);
        assert_eq!(bank.samples.len(), 2, "press and release clicks");
        assert_eq!(bank.variation, Variation::default());
        assert_eq!(preview.unwrap().len(), (0.07 * 44_100.0) as usize);
        let Picked { playing, failures, .. } = load_first(&[], 48_000, fake_load);
        assert!(playing.is_none() && failures.is_empty());
    }

    #[test]
    fn queued_requests_are_coalesced() {
        let (tx, rx) = mpsc::channel();
        for generation in 1..=3 {
            tx.send(Job::Bank(BankJob { generation, rate: 1000, candidates: vec![info("a")] }))
                .unwrap();
        }
        tx.send(Job::Preview(PreviewJob { info: info("p1"), rate: 1000 })).unwrap();
        tx.send(Job::Preview(PreviewJob { info: info("bad-p"), rate: 1000 })).unwrap();
        drop(tx);
        let results = Arc::new(Mutex::new(Vec::new()));
        let sink = results.clone();
        serve(
            rx,
            move |done| {
                sink.lock().unwrap().push(match done {
                    Done::Bank(b) => format!("bank {} {:?}", b.generation, b.playing.map(|p| p.id)),
                    Done::Preview(p) => {
                        format!("preview {} {} {}", p.id, p.clip.is_some(), p.error.is_some())
                    }
                })
            },
            fake_load,
        );
        assert_eq!(*results.lock().unwrap(), ["bank 3 Some(\"a\")", "preview bad-p false true"]);
    }
}
