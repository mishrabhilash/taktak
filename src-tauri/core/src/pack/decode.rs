//! Audio decoding (WAV / Ogg Vorbis / MP3 via symphonia), mono downmix, high-quality
//! resampling to the device rate, and leading-silence trimming. Runs on the control thread at
//! pack load, never on the audio thread.

use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Async, FixedAsync, Indexing, Resampler, SincInterpolationParameters};
use std::cell::RefCell;
use std::io::{Cursor, ErrorKind};
use symphonia::core::codecs::audio::well_known::{
    CODEC_ID_MP3, CODEC_ID_PCM_F32LE, CODEC_ID_PCM_S16LE, CODEC_ID_PCM_S24LE, CODEC_ID_PCM_S32LE,
    CODEC_ID_PCM_U8, CODEC_ID_VORBIS,
};
use symphonia::core::codecs::audio::{AudioCodecId, AudioDecoderOptions};
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

pub const MAX_SAMPLE_SECONDS: f32 = 2.0;
/// −50 dBFS.
pub const SILENCE_THRESHOLD: f32 = 0.003_162_3;
pub const PRE_ROLL_SECONDS: f32 = 0.0005;
pub const LEADING_SILENCE_WARN_SECONDS: f32 = 0.005;

/// Decoding stops once a file is this long. A file a little over [`MAX_SAMPLE_SECONDS`] still
/// gets its exact length reported (by `load`), but a 10 MB file of low-rate audio or low-bitrate
/// MP3 must not cost a full decode (tens of millions of samples) before it is rejected.
const MAX_DECODE_SECONDS: f32 = 2.0 * MAX_SAMPLE_SECONDS;

/// Sample rates outside this range are corrupt headers, not audio, and would make resampling
/// ratios (and buffers) absurd.
const SAMPLE_RATES: std::ops::RangeInclusive<u32> = 1_000..=1_000_000;

/// Windowed-sinc length (taps) and the resampler's processing chunk.
const SINC_LEN: usize = 256;
const RESAMPLE_CHUNK: usize = 1024;
/// Upper bound on the zero lead-in used to land the resampler delay on a whole output
/// sample. Common rate pairs need far less (44.1 kHz → 48 kHz: < 147 samples).
const MAX_ALIGN_PAD: usize = 2048;

/// Mono PCM at its native rate.
#[derive(Clone, Debug, PartialEq)]
pub struct Pcm {
    pub sample_rate: u32,
    pub samples: Vec<f32>,
}

impl Pcm {
    pub fn duration_seconds(&self) -> f32 {
        self.samples.len() as f32 / self.sample_rate.max(1) as f32
    }
}

/// Decodes a whole file. `extension` is the lowercase file extension (`"wav"`, `"ogg"`,
/// `"mp3"`), used as a format hint. Multichannel audio is averaged to mono. Errors are
/// human-readable ("not a valid Ogg Vorbis file: …").
pub fn decode(bytes: Vec<u8>, extension: &str) -> Result<Pcm, String> {
    let kind = format_name(extension);
    let invalid = |detail: String| format!("not a valid {kind} file: {detail}");
    if bytes.is_empty() {
        return Err(invalid("the file is empty".into()));
    }

    let mss = MediaSourceStream::new(Box::new(Cursor::new(bytes)), Default::default());
    let mut hint = Hint::new();
    hint.with_extension(extension);
    let mut format = symphonia::default::get_probe()
        .probe(&hint, mss, FormatOptions::default(), MetadataOptions::default())
        .map_err(|e| invalid(describe(&e)))?;

    // The probe goes by content, with the extension only as a hint; the format promises that
    // the extension names the container.
    let info = format.format_info();
    if !container_matches(extension, info.short_name) {
        return Err(format!(
            "the file extension .{extension} does not match its content ({}); rename the file \
             or convert it",
            info.long_name
        ));
    }

    let track =
        format.default_track(TrackType::Audio).ok_or_else(|| invalid("no audio track".into()))?;
    let track_id = track.id;
    let params = track
        .codec_params
        .as_ref()
        .and_then(|p| p.audio())
        .ok_or_else(|| invalid("no audio track".into()))?;
    let mut sample_rate = params.sample_rate.unwrap_or(0);
    let unsupported = || {
        format!(
            "unsupported audio codec in {kind} file; use PCM WAV (8/16/24/32-bit or 32-bit \
             float), Ogg Vorbis or MP3"
        )
    };
    // The symphonia features decode more than the format allows (64-bit float, A-law, μ-law).
    if !SUPPORTED_CODECS.contains(&params.codec) {
        return Err(unsupported());
    }
    // Gapless is on by default: encoder delay/padding (e.g. MP3's ~1105 leading samples) is
    // trimmed, which matters because leading silence is added keypress latency.
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(params, &AudioDecoderOptions::default())
        .map_err(|_| unsupported())?;

    let mut mono: Vec<f32> = Vec::new();
    let mut interleaved: Vec<f32> = Vec::new();
    let mut rate_known = false;
    let mut last_packet_error = None;
    loop {
        let packet = match format.next_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) => break,
            // Chained Ogg streams: keep the first one.
            Err(SymError::ResetRequired) => break,
            // Truncated file (common with sloppy WAV writers): keep what was decoded.
            Err(SymError::IoError(e)) if e.kind() == ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(invalid(describe(&e))),
        };
        if packet.track_id != track_id {
            continue;
        }
        let buf = match decoder.decode(&packet) {
            Ok(buf) => buf,
            // A corrupt packet is skipped, as players do; the file fails only if nothing decodes.
            Err(e @ (SymError::DecodeError(_) | SymError::IoError(_))) => {
                last_packet_error = Some(describe(&e));
                continue;
            }
            Err(e) => return Err(invalid(describe(&e))),
        };
        let channels = buf.num_planes();
        if channels == 0 || buf.frames() == 0 {
            continue;
        }
        let rate = buf.spec().rate();
        if !rate_known {
            if !SAMPLE_RATES.contains(&rate) {
                return Err(invalid(format!("unsupported sample rate {rate} Hz")));
            }
            sample_rate = rate;
            rate_known = true;
        } else if rate != sample_rate {
            return Err(invalid("the sample rate changes within the file".into()));
        }
        if mono.len() + buf.frames() > max_decoded_samples(sample_rate) {
            return Err(format!(
                "sound is over {MAX_DECODE_SECONDS:.2} s long; sounds must be at most \
                 {MAX_SAMPLE_SECONDS} s"
            ));
        }
        buf.copy_to_vec_interleaved(&mut interleaved);
        let scale = 1.0 / channels as f32;
        mono.extend(interleaved.chunks_exact(channels).map(|frame| {
            let s = frame.iter().sum::<f32>() * scale;
            // A single NaN/inf would poison every voice it is mixed with.
            if s.is_finite() { s } else { 0.0 }
        }));
    }

    if mono.is_empty() {
        return Err(match last_packet_error {
            Some(detail) => invalid(detail),
            None => format!("{kind} file contains no audio"),
        });
    }
    // The rate was checked when the first audio arrived.
    Ok(Pcm { sample_rate, samples: mono })
}

/// Samples of [`MAX_DECODE_SECONDS`] at `sample_rate` (at most 4 M, at the highest rate).
fn max_decoded_samples(sample_rate: u32) -> usize {
    (f64::from(MAX_DECODE_SECONDS) * f64::from(sample_rate)) as usize
}

/// The codecs `docs/pack-format.md` lists: PCM WAV (8/16/24/32-bit int, 32-bit float; symphonia
/// reports 8-bit WAV as unsigned), Vorbis and MP3. Only the WAV reader produces PCM ids and only
/// the Ogg reader Vorbis, so with [`container_matches`] each container gets its own codecs.
const SUPPORTED_CODECS: [AudioCodecId; 7] = [
    CODEC_ID_PCM_U8,
    CODEC_ID_PCM_S16LE,
    CODEC_ID_PCM_S24LE,
    CODEC_ID_PCM_S32LE,
    CODEC_ID_PCM_F32LE,
    CODEC_ID_VORBIS,
    CODEC_ID_MP3,
];

/// Whether symphonia's format reader (`short_name`) is the container the extension names.
/// MPEG audio layers 1 and 2 (`"mp1"`, `"mp2"`) are not MP3.
fn container_matches(extension: &str, short_name: &str) -> bool {
    matches!((extension, short_name), ("wav", "wave") | ("ogg", "ogg") | ("mp3", "mp3"))
}

fn format_name(extension: &str) -> &'static str {
    match extension {
        "wav" => "WAV",
        "ogg" => "Ogg Vorbis",
        "mp3" => "MP3",
        _ => "audio",
    }
}

fn describe(e: &SymError) -> String {
    match e {
        SymError::IoError(io) if io.kind() == ErrorKind::UnexpectedEof => {
            "the file is truncated".into()
        }
        SymError::IoError(io) => io.to_string(),
        SymError::DecodeError(msg) => format!("malformed data ({msg})"),
        SymError::Unsupported(msg) if msg.contains("no suitable format reader") => {
            "unrecognized audio format".into()
        }
        SymError::Unsupported(msg) => format!("unsupported feature ({msg})"),
        SymError::LimitError(msg) => format!("exceeds a decoder limit ({msg})"),
        other => other.to_string(),
    }
}

/// Band-limited resampling to `target_rate` (identity when rates match). Output length is
/// `round(len * target / source)`.
///
/// Windowed-sinc interpolation (256 taps, Blackman-Harris², automatic cutoff) via rubato. The
/// filter delay is removed exactly, so the output is time-aligned with the input: no added
/// leading silence (which would be keypress latency) and the tail is flushed, not cut.
pub fn resample(pcm: &Pcm, target_rate: u32) -> Vec<f32> {
    let source_rate = pcm.sample_rate;
    // A zero rate has no meaningful conversion; `decode` never produces one.
    if source_rate == target_rate || source_rate == 0 || target_rate == 0 {
        return pcm.samples.clone();
    }
    let len = pcm.samples.len();
    let out_len = resampled_len(len, source_rate, target_rate);
    if out_len == 0 {
        return Vec::new();
    }

    // The sinc resampler delays its input by SINC_LEN/2 input samples minus one output sample
    // (rubato 5's Async sinc: its first output sits one step after the window start; the
    // alignment test pins this down). That is usually a fractional number of output samples,
    // so prepend `pad` zeros chosen to make it whole, then drop exactly that many output
    // samples. Ratio = up/down in lowest terms.
    let g = gcd(source_rate, target_rate);
    let (up, down) = ((target_rate / g) as usize, (source_rate / g) as usize);
    let half = SINC_LEN / 2;
    let pad = if down <= MAX_ALIGN_PAD { (down - half % down) % down } else { 0 };
    let ratio = target_rate as f64 / source_rate as f64;
    let delay = if (pad + half).is_multiple_of(down) {
        ((pad + half) / down * up).saturating_sub(1)
    } else {
        // Exotic rate pair: round to the nearest output sample (≤ 0.5 sample error).
        ((pad + half) as f64 * ratio - 1.0).round().max(0.0) as usize
    };

    let mut input = Vec::with_capacity(pad + len);
    input.resize(pad, 0.0);
    input.extend_from_slice(&pcm.samples);

    let rates = (source_rate, target_rate);
    RESAMPLER.with(|cell| {
        let mut cached = cell.borrow_mut();
        if cached.as_ref().is_some_and(|c| c.rates != rates) {
            *cached = None;
        }
        let c = cached.get_or_insert_with(|| CachedResampler::new(rates));
        c.resampler.reset();
        run(&mut c.resampler, &input, delay, out_len)
    })
}

struct CachedResampler {
    rates: (u32, u32),
    resampler: Async<f32>,
}

impl CachedResampler {
    fn new(rates: (u32, u32)) -> CachedResampler {
        let ratio = rates.1 as f64 / rates.0 as f64;
        let params = SincInterpolationParameters::default().sinc_len(SINC_LEN);
        let resampler =
            Async::<f32>::new_sinc(ratio, 1.0, &params, RESAMPLE_CHUNK, 1, FixedAsync::Input)
                .expect("ratio is finite and positive; chunk size and channel count are non-zero");
        CachedResampler { rates, resampler }
    }
}

thread_local! {
    /// Building the sinc tables costs about as much as resampling a whole clip, and a pack
    /// load resamples every file with the same rate pair, so the last resampler is reused.
    static RESAMPLER: RefCell<Option<CachedResampler>> = const { RefCell::new(None) };
}

/// Pushes `input` (then silence, to flush the filter tail) through the resampler until
/// `skip + out_len` samples exist, and returns the `out_len` after the first `skip`.
fn run(resampler: &mut Async<f32>, input: &[f32], skip: usize, out_len: usize) -> Vec<f32> {
    let needed = skip + out_len;
    let mut out = vec![0.0f32; needed + resampler.output_frames_max()];
    let in_adapter = InterleavedSlice::new(input, 1, input.len())
        .expect("adapter length matches the input slice");
    let out_frames = out.len();
    let mut out_adapter = InterleavedSlice::new_mut(&mut out, 1, out_frames)
        .expect("adapter length matches the output slice");
    let (mut in_pos, mut out_pos) = (0, 0);
    while out_pos < needed {
        let chunk = resampler.input_frames_next();
        let available = (input.len() - in_pos).min(chunk);
        let mut indexing = Indexing::new().input_offset(in_pos).output_offset(out_pos);
        if available < chunk {
            indexing = indexing.partial_len(available);
        }
        let (_, written) = resampler
            .process_into_buffer(&in_adapter, &mut out_adapter, Some(&indexing))
            .expect("buffers are sized from input_frames_next and output_frames_max");
        in_pos += available;
        out_pos += written;
    }
    out.truncate(needed);
    out.drain(..skip);
    out
}

/// The length [`resample`] returns for `len` samples at `from` Hz resampled to `to` Hz:
/// `round(len * to / from)` without overflow (`len` when either rate is zero).
pub fn resampled_len(len: usize, from: u32, to: u32) -> usize {
    if from == 0 || to == 0 {
        return len;
    }
    let (len, from, to) = (len as u128, from as u128, to as u128);
    ((len * to * 2 + from) / (from * 2)) as usize
}

fn gcd(mut a: u32, mut b: u32) -> u32 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

/// Number of leading samples strictly below `SILENCE_THRESHOLD` in absolute value.
pub fn leading_silence(samples: &[f32]) -> usize {
    samples.iter().position(|s| s.abs() >= SILENCE_THRESHOLD).unwrap_or(samples.len())
}

/// Removes leading silence, keeping `PRE_ROLL_SECONDS` before the first audible sample.
/// Returns the number of samples removed. An all-silent buffer is left unchanged.
pub fn trim_leading_silence(samples: &mut Vec<f32>, sample_rate: u32) -> usize {
    let silent = leading_silence(samples);
    if silent == samples.len() {
        return 0;
    }
    let pre_roll = (PRE_ROLL_SECONDS * sample_rate as f32).round() as usize;
    let cut = silent.saturating_sub(pre_roll);
    samples.drain(..cut);
    cut
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    fn fixture(name: &str) -> Vec<u8> {
        let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
    }

    fn sine(len: usize, rate: u32, freq: f32, amp: f32) -> Vec<f32> {
        (0..len).map(|i| amp * (2.0 * PI * freq * i as f32 / rate as f32).sin()).collect()
    }

    /// Writes an in-memory WAV with hound. `interleaved` holds values in [-1, 1].
    fn wav(channels: u16, rate: u32, bits: u16, float: bool, interleaved: &[f32]) -> Vec<u8> {
        let spec = hound::WavSpec {
            channels,
            sample_rate: rate,
            bits_per_sample: bits,
            sample_format: if float {
                hound::SampleFormat::Float
            } else {
                hound::SampleFormat::Int
            },
        };
        let mut cursor = Cursor::new(Vec::new());
        let mut w = hound::WavWriter::new(&mut cursor, spec).unwrap();
        for &s in interleaved {
            let s64 = s as f64;
            match (float, bits) {
                (true, 32) => w.write_sample(s).unwrap(),
                (false, 8) => w.write_sample((s64 * 127.0).round() as i8).unwrap(),
                (false, 16) => w.write_sample((s64 * 32767.0).round() as i16).unwrap(),
                (false, 24) => w.write_sample((s64 * 8_388_607.0).round() as i32).unwrap(),
                (false, 32) => w.write_sample((s64 * 2_147_483_647.0).round() as i32).unwrap(),
                _ => unreachable!(),
            }
        }
        w.finalize().unwrap();
        cursor.into_inner()
    }

    /// A minimal RIFF/WAVE file with a plain (non-extensible) `fmt ` chunk.
    fn raw_wav(format_tag: u16, channels: u16, rate: u32, bits: u16, data: &[u8]) -> Vec<u8> {
        let block_align = channels * bits / 8;
        let mut fmt = Vec::new();
        fmt.extend_from_slice(&format_tag.to_le_bytes());
        fmt.extend_from_slice(&channels.to_le_bytes());
        fmt.extend_from_slice(&rate.to_le_bytes());
        fmt.extend_from_slice(&(rate * block_align as u32).to_le_bytes());
        fmt.extend_from_slice(&block_align.to_le_bytes());
        fmt.extend_from_slice(&bits.to_le_bytes());
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&((4 + 8 + fmt.len() + 8 + data.len()) as u32).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&(fmt.len() as u32).to_le_bytes());
        out.extend_from_slice(&fmt);
        out.extend_from_slice(b"data");
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(data);
        out
    }

    /// `raw_wav` output with the 2-byte `cbSize` field (0) that non-PCM `fmt ` chunks carry.
    fn with_cb_size(mut wav: Vec<u8>) -> Vec<u8> {
        wav.splice(36..36, [0, 0]);
        wav[16..20].copy_from_slice(&18u32.to_le_bytes());
        let riff = u32::from_le_bytes([wav[4], wav[5], wav[6], wav[7]]) + 2;
        wav[4..8].copy_from_slice(&riff.to_le_bytes());
        wav
    }

    fn max_abs_diff(a: &[f32], b: &[f32]) -> f32 {
        assert_eq!(a.len(), b.len());
        a.iter().zip(b).map(|(x, y)| (x - y).abs()).fold(0.0, f32::max)
    }

    fn peak(v: &[f32]) -> f32 {
        v.iter().fold(0.0f32, |m, s| m.max(s.abs()))
    }

    #[test]
    fn wav_int_depths_and_float_decode_accurately() {
        let signal = sine(441, 44_100, 1000.0, 0.8);
        for (bits, float, tolerance) in [
            (8, false, 1.0 / 64.0),
            (16, false, 1e-4),
            (24, false, 1e-6),
            (32, false, 1e-6),
            (32, true, 0.0),
        ] {
            let pcm = decode(wav(1, 44_100, bits, float, &signal), "wav")
                .unwrap_or_else(|e| panic!("{bits}-bit float={float}: {e}"));
            assert_eq!(pcm.sample_rate, 44_100);
            assert_eq!(pcm.samples.len(), signal.len(), "{bits}-bit float={float}");
            let err = max_abs_diff(&pcm.samples, &signal);
            assert!(err <= tolerance, "{bits}-bit float={float}: error {err}");
        }
    }

    #[test]
    fn plain_ieee_float_wav_decodes() {
        // hound writes WAVE_FORMAT_EXTENSIBLE for float; many tools write tag 3 directly.
        let signal = [0.0f32, 0.5, -0.25, 1.0, -1.0];
        let data: Vec<u8> = signal.iter().flat_map(|s| s.to_le_bytes()).collect();
        let pcm = decode(raw_wav(3, 1, 48_000, 32, &data), "wav").unwrap();
        assert_eq!(pcm, Pcm { sample_rate: 48_000, samples: signal.to_vec() });
    }

    #[test]
    fn stereo_and_multichannel_are_averaged_to_mono() {
        let frames = 100;
        let stereo: Vec<f32> = (0..frames).flat_map(|_| [0.5, -0.25]).collect();
        let pcm = decode(wav(2, 48_000, 16, false, &stereo), "wav").unwrap();
        assert_eq!(pcm.samples.len(), frames);
        assert!(pcm.samples.iter().all(|s| (s - 0.125).abs() < 1e-4), "{:?}", &pcm.samples[..4]);

        let quad: Vec<f32> = (0..frames).flat_map(|_| [0.8, 0.4, 0.0, -0.4]).collect();
        let pcm = decode(wav(4, 48_000, 24, false, &quad), "wav").unwrap();
        assert_eq!(pcm.samples.len(), frames);
        assert!(pcm.samples.iter().all(|s| (s - 0.2).abs() < 1e-5));
    }

    #[test]
    fn sample_rate_is_preserved() {
        for rate in [8_000, 11_025, 22_050, 32_000, 44_100, 48_000, 88_200, 96_000, 192_000] {
            let pcm =
                decode(wav(1, rate, 16, false, &sine(rate as usize / 5, rate, 440.0, 0.5)), "wav")
                    .unwrap();
            assert_eq!(pcm.sample_rate, rate);
            assert_eq!(pcm.samples.len(), rate as usize / 5);
            assert!((pcm.duration_seconds() - 0.2).abs() < 1e-4, "{rate}");
        }
    }

    #[test]
    fn non_finite_float_samples_become_silence() {
        let data: Vec<u8> =
            [0.5f32, f32::NAN, f32::INFINITY, -0.5].iter().flat_map(|s| s.to_le_bytes()).collect();
        let pcm = decode(raw_wav(3, 1, 44_100, 32, &data), "wav").unwrap();
        assert_eq!(pcm.samples, vec![0.5, 0.0, 0.0, -0.5]);
    }

    #[test]
    fn truncated_wav_keeps_the_decoded_part() {
        let mut bytes = wav(1, 44_100, 16, false, &sine(4410, 44_100, 440.0, 0.5));
        bytes.truncate(bytes.len() - 1000);
        let pcm = decode(bytes, "wav").unwrap();
        assert!(pcm.samples.len() > 3000 && pcm.samples.len() < 4410, "{}", pcm.samples.len());
    }

    #[test]
    fn invalid_input_gives_readable_errors() {
        let err = decode(Vec::new(), "wav").unwrap_err();
        assert_eq!(err, "not a valid WAV file: the file is empty");

        let err = decode(b"definitely not audio, just some text".repeat(20), "ogg").unwrap_err();
        assert!(err.starts_with("not a valid Ogg Vorbis file: "), "{err}");

        let err = decode(b"RIFF\x10\0\0\0WAVEjunk".to_vec(), "wav").unwrap_err();
        assert!(err.starts_with("not a valid WAV file: "), "{err}");

        let err = decode(raw_wav(1, 1, 44_100, 16, &[]), "wav").unwrap_err();
        assert_eq!(err, "WAV file contains no audio");

        let err = decode(raw_wav(1, 1, 500, 16, &[0; 64]), "wav").unwrap_err();
        assert_eq!(err, "not a valid WAV file: unsupported sample rate 500 Hz");

        // An unknown WAVE format tag (not PCM or float).
        let err = decode(raw_wav(0x7777, 1, 44_100, 16, &[0; 64]), "wav").unwrap_err();
        assert!(err.contains("WAV") && err.contains("unsupported"), "{err}");
    }

    #[test]
    fn decoding_stops_well_short_of_a_huge_file() {
        // 10 MB of 8-bit audio at 1 kHz: 10 M samples, which decoding must not run through.
        let huge = raw_wav(1, 1, 1_000, 8, &vec![0x80; 10 * 1024 * 1024]);
        let err = decode(huge, "wav").unwrap_err();
        assert_eq!(err, "sound is over 4.00 s long; sounds must be at most 2 s");
        // A file a little over the limit decodes in full, so `load` can say how long it is.
        let pcm = decode(raw_wav(1, 1, 1_000, 8, &[0x80; 3_000]), "wav").unwrap();
        assert_eq!(pcm.samples.len(), 3_000);
        let pcm = decode(raw_wav(1, 1, 1_000, 8, &[0x80; 4_000]), "wav").unwrap();
        assert_eq!(pcm.samples.len(), 4_000);
        assert!(decode(raw_wav(1, 1, 1_000, 8, &[0x80; 4_001]), "wav").is_err());
    }

    #[test]
    fn wav_codecs_outside_the_spec_are_rejected() {
        // symphonia decodes these, but the format lists only 8/16/24/32-bit int and 32-bit float.
        let f64_data: Vec<u8> = [0.5f64, -0.5].iter().flat_map(|s| s.to_le_bytes()).collect();
        for (what, bytes) in [
            ("64-bit float", raw_wav(3, 1, 44_100, 64, &f64_data)),
            ("A-law", with_cb_size(raw_wav(6, 1, 8_000, 8, &[0x55; 64]))),
            ("mu-law", with_cb_size(raw_wav(7, 1, 8_000, 8, &[0xff; 64]))),
        ] {
            let err = decode(bytes, "wav").unwrap_err();
            assert!(err.starts_with("unsupported audio codec in WAV file"), "{what}: {err}");
        }
        // The allowed ones still decode, plain or extensible.
        assert!(decode(raw_wav(1, 1, 44_100, 8, &[0x80, 0xff, 0x00, 0x80]), "wav").is_ok());
        assert!(decode(wav(1, 44_100, 32, true, &[0.5, -0.5]), "wav").is_ok());
    }

    #[test]
    fn extension_must_name_the_container() {
        let wav_bytes = wav(1, 44_100, 16, false, &sine(441, 44_100, 1000.0, 0.5));
        let cases = [
            (fixture("tone-click-mono-44100.mp3"), "wav", "MPEG Audio Layer 3"),
            (fixture("tone-click-stereo-44100.ogg"), "wav", "Ogg"),
            (fixture("tone-click-stereo-44100.ogg"), "mp3", "Ogg"),
            (wav_bytes.clone(), "ogg", "Waveform"),
            (wav_bytes.clone(), "mp3", "Waveform"),
        ];
        for (bytes, extension, content) in cases {
            let err = decode(bytes, extension).unwrap_err();
            assert!(
                err.starts_with(&format!("the file extension .{extension} does not match")),
                "{extension}: {err}"
            );
            assert!(err.contains(content), "{extension}: {err}");
        }
        assert!(decode(wav_bytes, "wav").is_ok());
    }

    #[test]
    fn mp3_mono_decodes_with_encoder_delay_removed() {
        // 0.1 s: 0.5 * sin(440 Hz) plus a 0.4 click during the first 2 ms, 44.1 kHz mono.
        let pcm = decode(fixture("tone-click-mono-44100.mp3"), "mp3").unwrap();
        assert_eq!(pcm.sample_rate, 44_100);
        assert!((pcm.samples.len() as i64 - 4410).abs() <= 1, "len {}", pcm.samples.len());
        let p = peak(&pcm.samples);
        assert!((0.7..1.1).contains(&p), "peak {p}");
        // Gapless trimming removed the ~1105-sample encoder delay: the click is at the start.
        let lead = leading_silence(&pcm.samples);
        assert!(lead < 44, "leading silence {lead} samples");
    }

    #[test]
    fn mp3_stereo_with_id3_tag_decodes_and_downmixes() {
        // 48 kHz stereo, left = 0.6 * sin(440 Hz) + 0.3 click, right = silence, ID3v2 title.
        let pcm = decode(fixture("tone-click-stereo-48000.mp3"), "mp3").unwrap();
        assert_eq!(pcm.sample_rate, 48_000);
        assert!((pcm.samples.len() as i64 - 4800).abs() <= 1, "len {}", pcm.samples.len());
        let p = peak(&pcm.samples);
        assert!((0.35..0.6).contains(&p), "peak {p} (expected about half of 0.9)");
        assert!(leading_silence(&pcm.samples) < 48);
    }

    #[test]
    fn ogg_vorbis_stereo_decodes_and_downmixes() {
        // 44.1 kHz stereo, left = 0.6 * sin(440 Hz) + 0.3 click, right = silence.
        let pcm = decode(fixture("tone-click-stereo-44100.ogg"), "ogg").unwrap();
        assert_eq!(pcm.sample_rate, 44_100);
        assert!((pcm.samples.len() as i64 - 4410).abs() <= 10, "len {}", pcm.samples.len());
        let p = peak(&pcm.samples);
        assert!((0.35..0.6).contains(&p), "peak {p} (expected about half of 0.9)");
        assert!(leading_silence(&pcm.samples) < 44);
    }

    #[test]
    fn leading_silence_counts_samples_below_threshold() {
        assert_eq!(leading_silence(&[]), 0);
        assert_eq!(leading_silence(&[0.0, 0.003, -0.003, 0.5]), 3);
        assert_eq!(leading_silence(&[0.0, -SILENCE_THRESHOLD, 0.0]), 1);
        assert_eq!(leading_silence(&[0.0, 0.001]), 2);
    }

    #[test]
    fn trim_keeps_pre_roll_before_first_audible_sample() {
        // 48 kHz: pre-roll is 24 samples.
        let mut s = vec![0.0; 100];
        s.extend([0.5, 0.25]);
        assert_eq!(trim_leading_silence(&mut s, 48_000), 76);
        assert_eq!(s.len(), 26);
        assert_eq!(s[24], 0.5);

        // Less silence than the pre-roll: nothing to cut.
        let mut s = vec![0.0, 0.0, 0.9];
        assert_eq!(trim_leading_silence(&mut s, 48_000), 0);
        assert_eq!(s, vec![0.0, 0.0, 0.9]);

        // All silent (and empty) buffers are left alone.
        let mut s = vec![0.001; 500];
        assert_eq!(trim_leading_silence(&mut s, 44_100), 0);
        assert_eq!(s.len(), 500);
        let mut s: Vec<f32> = Vec::new();
        assert_eq!(trim_leading_silence(&mut s, 44_100), 0);

        // 44.1 kHz: 22.05 rounds to 22 samples of pre-roll.
        let mut s = vec![0.0; 1000];
        s.push(-0.2);
        assert_eq!(trim_leading_silence(&mut s, 44_100), 978);
        assert_eq!(leading_silence(&s), 22);
    }

    #[test]
    fn resample_identity_and_degenerate_inputs() {
        let pcm = Pcm { sample_rate: 48_000, samples: sine(100, 48_000, 440.0, 0.5) };
        assert_eq!(resample(&pcm, 48_000), pcm.samples);
        let empty = Pcm { sample_rate: 44_100, samples: Vec::new() };
        assert!(resample(&empty, 48_000).is_empty());
    }

    #[test]
    fn resample_output_length_is_rounded_ratio() {
        for (from, to) in [
            (44_100, 48_000),
            (48_000, 44_100),
            (96_000, 48_000),
            (22_050, 48_000),
            (44_100, 47_999),
            (8_000, 192_000),
        ] {
            for len in [1usize, 2, 10, 147, 1000, 4410, 9999] {
                let pcm = Pcm { sample_rate: from, samples: sine(len, from, 300.0, 0.5) };
                let expected = (len as f64 * to as f64 / from as f64).round() as usize;
                assert_eq!(resample(&pcm, to).len(), expected, "{from}->{to} len {len}");
            }
        }
    }

    #[test]
    fn resample_short_input_keeps_its_energy() {
        // 10 samples: shorter than the filter. The click must survive, not vanish.
        let mut x = vec![0.0f32; 10];
        x[3] = 1.0;
        let y = resample(&Pcm { sample_rate: 44_100, samples: x }, 48_000);
        assert_eq!(y.len(), 11);
        let (i, &p) = y.iter().enumerate().max_by(|a, b| a.1.abs().total_cmp(&b.1.abs())).unwrap();
        assert!(p > 0.5, "peak {p}");
        assert!((i as f64 - 3.0 * 48_000.0 / 44_100.0).abs() <= 1.0, "peak at {i}");
    }

    fn gaussian(len: usize, rate: u32, center_s: f64, sigma_s: f64) -> Vec<f32> {
        (0..len)
            .map(|i| {
                let t = (i as f64 / rate as f64 - center_s) / sigma_s;
                (0.8 * (-t * t).exp()) as f32
            })
            .collect()
    }

    fn centroid(v: &[f32]) -> f64 {
        let sum: f64 = v.iter().map(|&s| s as f64).sum();
        v.iter().enumerate().map(|(i, &s)| i as f64 * s as f64).sum::<f64>() / sum
    }

    #[test]
    fn resample_is_time_aligned_without_added_latency() {
        // A smooth (band-limited) pulse must come out where the same pulse sampled at the
        // target rate would be: same shape, same time. Exact for common rate pairs.
        let cases = [
            (44_100, 48_000, 0.02),
            (48_000, 44_100, 0.02),
            (96_000, 48_000, 0.02),
            (22_050, 48_000, 0.02),
            (48_000, 96_000, 0.02),
            (44_100, 47_999, 0.55), // exotic pair: nearest-sample alignment
        ];
        for (from, to, tolerance) in cases {
            for center_ms in [1.0, 3.3, 20.0] {
                let (center, sigma) = (center_ms / 1000.0, 0.000_25);
                let x = gaussian((from / 20) as usize, from, center, sigma);
                let y = resample(&Pcm { sample_rate: from, samples: x }, to);
                let expected = gaussian(y.len(), to, center, sigma);
                let shift = centroid(&y) - centroid(&expected);
                println!("{from}->{to} pulse at {center_ms} ms: shift {shift:+.4} samples");
                assert!(shift.abs() <= tolerance, "{from}->{to} at {center_ms} ms: {shift}");
                if tolerance < 0.1 {
                    let err = max_abs_diff(&y, &expected);
                    assert!(err < 2e-3, "{from}->{to} at {center_ms} ms: shape error {err}");
                }
            }
        }
    }

    #[test]
    fn resample_impulse_stays_at_the_same_time() {
        for (from, to) in [(44_100, 48_000), (48_000, 44_100), (96_000, 48_000)] {
            for p in [0usize, 5, 100, 1001] {
                let mut x = vec![0.0f32; 4000];
                x[p] = 1.0;
                let y = resample(&Pcm { sample_rate: from, samples: x }, to);
                let (i, _) =
                    y.iter().enumerate().max_by(|a, b| a.1.abs().total_cmp(&b.1.abs())).unwrap();
                let expected = p as f64 * to as f64 / from as f64;
                assert!(
                    (i as f64 - expected).abs() <= 1.0,
                    "{from}->{to} p={p}: {i} vs {expected}"
                );
            }
        }
    }

    #[test]
    fn resample_sharp_onset_has_no_leading_pre_ringing() {
        // Onset at t = 0 (what trimming produces, minus the pre-roll): the output must be loud
        // from its first sample, with nothing ringing ahead of it.
        for (from, to) in [(44_100, 48_000), (48_000, 44_100), (96_000, 48_000)] {
            let len = from as usize / 20;
            let x: Vec<f32> = (0..len)
                .map(|i| {
                    let t = i as f32 / from as f32;
                    0.8 * (2.0 * PI * 2000.0 * t).cos() * (-t / 0.005).exp()
                })
                .collect();
            let y = resample(&Pcm { sample_rate: from, samples: x.clone() }, to);
            assert_eq!(leading_silence(&y), 0, "{from}->{to}");
            // Upsampling passes through the input samples; downsampling low-passes the step,
            // which puts it half-way up at t = 0. Either way the very first sample is loud.
            assert!(y[0] >= 0.5 * x[0], "{from}->{to}: first sample {} vs {}", y[0], x[0]);

            // Same onset 10 ms in. Any linear-phase band-limited resampler rings ahead of a
            // full-scale step (Gibbs); measure how much and how far.
            let k = from as usize / 100;
            let mut delayed = vec![0.0; k];
            delayed.extend_from_slice(&x);
            let y = resample(&Pcm { sample_rate: from, samples: delayed.clone() }, to);
            let onset = (k as f64 * to as f64 / from as f64).round() as usize;
            let audible_ahead = onset - leading_silence(&y).min(onset);
            let ring_db = 20.0 * (peak(&y[..onset - 1]) / 0.8).log10();
            println!(
                "{from}->{to}: pre-ringing peak {ring_db:.1} dB re onset, above -50 dBFS from \
                 {audible_ahead} samples ({:.3} ms) ahead",
                audible_ahead as f64 * 1000.0 / to as f64
            );
            assert!(ring_db < -15.0, "{from}->{to}: {ring_db} dB");
            assert!(audible_ahead <= 40, "{from}->{to}: {audible_ahead} samples");

            // In the load pipeline (trim, then resample) ringing ahead of the kept pre-roll would
            // fall before t = 0 and is dropped: no added latency, onset where it was.
            let mut trimmed = delayed;
            trim_leading_silence(&mut trimmed, from);
            let y = resample(&Pcm { sample_rate: from, samples: trimmed }, to);
            let pre_roll = (PRE_ROLL_SECONDS * to as f32).round() as usize;
            let (loudest, _) =
                y.iter().enumerate().max_by(|a, b| a.1.abs().total_cmp(&b.1.abs())).unwrap();
            assert!(loudest.abs_diff(pre_roll) <= 1, "{from}->{to}: onset at {loudest}");
        }
    }

    #[test]
    fn resample_preserves_sine_amplitude() {
        for (from, to) in [(44_100, 48_000), (48_000, 44_100), (96_000, 48_000), (22_050, 48_000)] {
            for freq in [100.0, 1000.0, 8000.0] {
                let y = resample(
                    &Pcm { sample_rate: from, samples: sine(from as usize / 5, from, freq, 0.5) },
                    to,
                );
                // Ignore the filter's edge transients.
                let mid = &y[y.len() / 4..3 * y.len() / 4];
                let rms = (mid.iter().map(|s| s * s).sum::<f32>() / mid.len() as f32).sqrt();
                let expected = 0.5 / 2f32.sqrt();
                assert!((rms / expected - 1.0).abs() < 0.005, "{from}->{to} {freq} Hz: rms {rms}");
                // Same phase too, allowing for rubato's fixed 1/128-input-sample table offset
                // (≤ 0.02 output samples, see the alignment test).
                let expected_wave = sine(y.len(), to, freq, 0.5);
                let err = max_abs_diff(mid, &expected_wave[y.len() / 4..3 * y.len() / 4]);
                let allowed = 0.5 * 2.0 * PI * freq * 0.02 / to as f32 + 0.001;
                assert!(err < allowed, "{from}->{to} {freq} Hz: waveform error {err}");
            }
        }
    }

    #[test]
    fn resample_reuses_cached_resampler_deterministically() {
        let a = Pcm { sample_rate: 44_100, samples: sine(3000, 44_100, 700.0, 0.4) };
        let b = Pcm { sample_rate: 96_000, samples: sine(3000, 96_000, 700.0, 0.4) };
        let first = resample(&a, 48_000);
        let _ = resample(&b, 48_000);
        assert_eq!(resample(&a, 48_000), first);
        assert_eq!(resample(&a, 48_000), first);
    }
}
