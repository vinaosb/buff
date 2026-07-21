//! `buff-audio` — audio codecs + sample ops for the Buff language.
//!
//! Pure-Rust MVP wrapping [`hound`] (WAV) + [`symphonia`] (MP3 / FLAC /
//! Vorbis). CPU-only per Metis G7 lock — no GPU dispatch, no real-time
//! playback (deferred to v1.18+), no synthesis (that's `buff-dsp` T11).
//!
//! # Pipeline
//!
//! ```text
//!   AudioBuffer.from_path(p)   ──┐
//!                                ▼
//!   AudioBuffer.from_samples(s) ─▶ AudioBuffer { samples: Vec<f32>,
//!                                │              sample_rate: u32,
//!                                │              channels: u16 }
//!                                │
//!                                ├─ a.samples() / sample_rate() / channels()
//!                                ├─ a.duration_secs() / frames()
//!                                ├─ a.amplify(factor)
//!                                ├─ a.normalize(target)
//!                                ├─ a.mix(other)
//!                                ├─ a.slice(start_sec, end_sec)
//!                                └─ a.save(path)
//! ```
//!
//! Samples are interleaved f32 in `-1.0..=1.0`, frame-major
//! (`[L0, R0, L1, R1, ...]` for stereo).
//!
//! # FFI safety
//!
//! Every public entry point follows the six hard rules from
//! `crates/buff-lang-ffi-guide/GUIDE.md`:
//!
//! | Rule | How this crate complies |
//! |------|-------------------------|
//! | R1 — No raw pointers | Public surface exposes only `AudioBuffer`, `AudioSummary`, `AudioError`. No `*const` / `*mut` anywhere. |
//! | R2 — Ownership boundary | `from_path` / `from_samples` return owned `AudioBuffer`. `samples()` borrows. `slice()` returns a new owned buffer. |
//! | R3 — Error mapping | Every fallible op returns `Result<T, AudioError>`. `hound::Error` + `symphonia::core::errors::Error` + `std::io::Error` mapped via `From`. |
//! | R4 — Thread safety | `AudioBuffer` is `Send + 'static` (owned `Vec<f32>` + two `Copy` fields). |
//! | R5 — Lifetime hiding | No public lifetime parameters anywhere. |
//! | R6 — Panic boundary | `from_path` / `save` wrap their bodies in `catch_unwind` (per FFI guide §6). |
//!
//! # Panic-free contract
//!
//! No `unwrap` / `expect` / `panic!` / `todo!` / `unimplemented!` in
//! non-test code. Bounds-checked ops return `Result`.

use std::fmt;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;

use thiserror::Error;

/// All fallible `buff-audio` operations return this error type.
///
/// Variants are intentionally string-carried (not structured) so the
/// future `BuffError` migration is a one-line change per call site.
/// Mirrors the `buff-image::ImageError` precedent.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum AudioError {
    #[error("audio I/O error: {0}")]
    Io(String),
    #[error("audio decode error: {0}")]
    Decode(String),
    #[error("audio encode error: {0}")]
    Encode(String),
    #[error("invalid audio parameter: {0}")]
    InvalidParam(String),
}

impl From<std::io::Error> for AudioError {
    fn from(e: std::io::Error) -> Self {
        AudioError::Io(e.to_string())
    }
}

impl From<hound::Error> for AudioError {
    fn from(e: hound::Error) -> Self {
        match e {
            hound::Error::IoError(io) => AudioError::Io(io.to_string()),
            other => AudioError::Decode(other.to_string()),
        }
    }
}

impl From<symphonia::core::errors::Error> for AudioError {
    fn from(e: symphonia::core::errors::Error) -> Self {
        AudioError::Decode(e.to_string())
    }
}

/// Owned interleaved f32 audio buffer.
///
/// Samples are stored as a flat `Vec<f32>` in `-1.0..=1.0` range,
/// interleaved across channels (frame-major: `[L0, R0, L1, R1, ...]`
/// for stereo). Sample rate is in Hz; channels is `>= 1`.
///
/// Construct via [`AudioBuffer::from_samples`] (programmatic) or
/// [`AudioBuffer::from_path`] (decode WAV/MP3/FLAC/Vorbis file).
#[derive(Debug, Clone, PartialEq)]
pub struct AudioBuffer {
    samples: Vec<f32>,
    sample_rate: u32,
    channels: u16,
}

impl AudioBuffer {
    /// Construct from already-interleaved samples.
    ///
    /// `samples.len()` must be a multiple of `channels` — otherwise
    /// returns [`AudioError::InvalidParam`].
    pub fn from_samples(
        samples: Vec<f32>,
        sample_rate: u32,
        channels: u16,
    ) -> Result<Self, AudioError> {
        if channels == 0 {
            return Err(AudioError::InvalidParam(
                "channels must be >= 1".to_string(),
            ));
        }
        if sample_rate == 0 {
            return Err(AudioError::InvalidParam(
                "sample_rate must be > 0".to_string(),
            ));
        }
        if !samples.len().is_multiple_of(channels as usize) {
            return Err(AudioError::InvalidParam(format!(
                "samples.len() ({}) must be a multiple of channels ({})",
                samples.len(),
                channels
            )));
        }
        Ok(Self {
            samples,
            sample_rate,
            channels,
        })
    }

    /// Decode any WAV/MP3/FLAC/Vorbis file into interleaved f32 samples.
    ///
    /// WAV is read via `hound` (faster, simpler, 100% coverage of WAV
    /// variants); all other formats fall back to `symphonia`. Format
    /// detection is by file extension first, then symphonia's
    /// content-sniffing `Probe` takes over.
    ///
    /// The body is wrapped in `catch_unwind` per FFI guide R6 so a
    /// panic inside either codec library becomes a structured
    /// [`AudioError::Decode`] instead of unwinding into Buff code.
    pub fn from_path<P: AsRef<Path>>(path: P) -> Result<Self, AudioError> {
        let p = path.as_ref().to_path_buf();
        let result = catch_unwind(AssertUnwindSafe(move || -> Result<Self, AudioError> {
            let ext = p
                .extension()
                .and_then(|e| e.to_str())
                .map(|s| s.to_ascii_lowercase())
                .unwrap_or_default();
            match ext.as_str() {
                "wav" | "wave" => Self::from_wav(&p),
                _ => Self::from_symphonia(&p, &ext),
            }
        }));
        match result {
            Ok(Ok(buf)) => Ok(buf),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(AudioError::Decode(
                "codec panicked during decode".to_string(),
            )),
        }
    }

    fn from_wav(path: &Path) -> Result<Self, AudioError> {
        let mut reader = hound::WavReader::open(path)?;
        let spec = reader.spec();
        let channels = spec.channels;
        let sample_rate = spec.sample_rate;

        let mut samples: Vec<f32> = match spec.sample_format {
            hound::SampleFormat::Int => match spec.bits_per_sample {
                8 => reader
                    .samples::<i8>()
                    .filter_map(Result::ok)
                    .map(|s| (s as f32) / (i8::MAX as f32))
                    .collect(),
                16 => reader
                    .samples::<i16>()
                    .filter_map(Result::ok)
                    .map(|s| (s as f32) / (i16::MAX as f32))
                    .collect(),
                24 => reader
                    .samples::<i32>()
                    .filter_map(Result::ok)
                    .map(|s| {
                        let v = s >> 8;
                        (v as f32) / ((1 << 23) as f32)
                    })
                    .collect(),
                32 => reader
                    .samples::<i32>()
                    .filter_map(Result::ok)
                    .map(|s| (s as f32) / (i32::MAX as f32))
                    .collect(),
                _ => reader
                    .samples::<i32>()
                    .filter_map(Result::ok)
                    .map(|s| (s as f32) / (i32::MAX as f32))
                    .collect(),
            },
            hound::SampleFormat::Float => reader.samples::<f32>().filter_map(Result::ok).collect(),
        };

        // Clamp to [-1.0, 1.0]: decoded integer samples can be out of range
        // due to two's-complement asymmetry (i16::MIN=-32768, i16::MAX=+32767).
        for s in samples.iter_mut() {
            *s = (*s).clamp(-1.0, 1.0);
        }

        samples.shrink_to_fit();
        Self::from_samples(samples, sample_rate, channels)
    }

    fn from_symphonia(path: &Path, ext: &str) -> Result<Self, AudioError> {
        use symphonia::core::codecs::CodecParameters;
        use symphonia::core::formats::probe::Hint;
        use symphonia::core::formats::FormatOptions;
        use symphonia::core::io::MediaSourceStream;
        use symphonia::core::meta::MetadataOptions;
        use symphonia::default::{get_codecs, get_probe};

        let file = std::fs::File::open(path)?;
        let mss = MediaSourceStream::new(Box::new(file), Default::default());

        let mut hint = Hint::new();
        if !ext.is_empty() {
            hint.with_extension(ext);
        }

        let mut format = get_probe().probe(
            &hint,
            mss,
            FormatOptions::default(),
            MetadataOptions::default(),
        )?;

        let track = format
            .tracks()
            .iter()
            .find(|t| matches!(t.codec_params, Some(CodecParameters::Audio(_))))
            .cloned()
            .ok_or_else(|| AudioError::Decode("no decodable audio track found".to_string()))?;

        let params = match track.codec_params {
            Some(CodecParameters::Audio(a)) => a,
            _ => {
                return Err(AudioError::Decode(
                    "selected track is not audio".to_string(),
                ))
            }
        };

        let sample_rate = params
            .sample_rate
            .ok_or_else(|| AudioError::Decode("unknown sample rate".to_string()))?;
        let channels = params.channels.as_ref().map(|c| c.count()).unwrap_or(1) as u16;

        let mut decoder = get_codecs().make_audio_decoder(&params, &Default::default())?;

        let mut samples: Vec<f32> = Vec::new();
        let track_id = track.id;

        loop {
            let packet = match format.next_packet() {
                Ok(Some(p)) => p,
                Ok(None) => break,
                Err(symphonia::core::errors::Error::IoError(ref e))
                    if e.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    break;
                }
                Err(symphonia::core::errors::Error::ResetRequired) => continue,
                Err(_) => break,
            };

            if packet.track_id != track_id {
                continue;
            }

            match decoder.decode(&packet) {
                Ok(decoded) => decoded.copy_to_vec_interleaved(&mut samples),
                Err(symphonia::core::errors::Error::DecodeError(_)) => continue,
                Err(_) => break,
            }
        }

        samples.shrink_to_fit();
        Self::from_samples(samples, sample_rate, channels)
    }

    /// Borrowed view of the interleaved samples (`-1.0..=1.0`).
    #[inline]
    pub fn samples(&self) -> &[f32] {
        &self.samples
    }

    /// Sample rate in Hz.
    #[inline]
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Channel count (`>= 1`).
    #[inline]
    pub fn channels(&self) -> u16 {
        self.channels
    }

    /// Number of frames (`samples.len() / channels`).
    #[inline]
    pub fn frames(&self) -> usize {
        if self.channels == 0 {
            0
        } else {
            self.samples.len() / self.channels as usize
        }
    }

    /// Duration in seconds (`frames / sample_rate`).
    pub fn duration_secs(&self) -> f64 {
        if self.sample_rate == 0 {
            0.0
        } else {
            self.frames() as f64 / self.sample_rate as f64
        }
    }

    /// Encode to a WAV file at `path`. 32-bit float WAV is written
    /// (lossless round-trip for our f32 samples).
    ///
    /// Wrapped in `catch_unwind` per FFI guide R6 so an encoder panic
    /// becomes a structured [`AudioError::Encode`] instead of
    /// unwinding into Buff code.
    pub fn save<P: AsRef<Path>>(&self, path: P) -> Result<(), AudioError> {
        if self.channels == 0 {
            return Err(AudioError::InvalidParam(
                "channels must be >= 1".to_string(),
            ));
        }
        let p = path.as_ref().to_path_buf();
        let result = catch_unwind(AssertUnwindSafe(|| -> Result<(), AudioError> {
            let spec = hound::WavSpec {
                channels: self.channels,
                sample_rate: self.sample_rate,
                bits_per_sample: 32,
                sample_format: hound::SampleFormat::Float,
            };
            let mut writer = hound::WavWriter::create(&p, spec)?;
            for &s in &self.samples {
                writer.write_sample(s.clamp(-1.0, 1.0))?;
            }
            writer.finalize()?;
            Ok(())
        }));
        match result {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(AudioError::Encode("encoder panicked".to_string())),
        }
    }

    /// Scale every sample by `factor` in place.
    pub fn amplify(&mut self, factor: f32) {
        for s in self.samples.iter_mut() {
            *s *= factor;
        }
    }

    /// Normalize so the peak absolute sample is `target` (default 1.0).
    /// If all samples are zero, the buffer is left unchanged. If
    /// `target` is non-finite or `<= 0.0`, defaults to `1.0`.
    pub fn normalize(&mut self, target: f32) {
        let target = if target.is_finite() && target > 0.0 {
            target
        } else {
            1.0
        };
        let peak: f32 = self.samples.iter().map(|s| s.abs()).fold(0.0_f32, f32::max);
        if peak <= 0.0 {
            return;
        }
        let gain = target / peak;
        for s in self.samples.iter_mut() {
            *s *= gain;
        }
    }

    /// Sample-wise mix: `self.samples[i] = self.samples[i] + other.samples[i]`.
    ///
    /// The two buffers MUST share `sample_rate` and `channels`. If
    /// `other` is shorter, the trailing samples of `self` are
    /// unchanged; if longer, the excess is dropped. Returns
    /// [`AudioError::InvalidParam`] on rate/channel mismatch.
    pub fn mix(&mut self, other: &AudioBuffer) -> Result<(), AudioError> {
        if self.sample_rate != other.sample_rate {
            return Err(AudioError::InvalidParam(format!(
                "sample_rate mismatch: {} vs {}",
                self.sample_rate, other.sample_rate
            )));
        }
        if self.channels != other.channels {
            return Err(AudioError::InvalidParam(format!(
                "channels mismatch: {} vs {}",
                self.channels, other.channels
            )));
        }
        let n = self.samples.len().min(other.samples.len());
        for i in 0..n {
            self.samples[i] += other.samples[i];
        }
        Ok(())
    }

    /// Return a new buffer containing samples from `start_sec` to
    /// `end_sec` (clamped to `[0, duration_secs()]`). Endpoints are
    /// rounded to the nearest frame boundary.
    pub fn slice(&self, start_sec: f64, end_sec: f64) -> Result<AudioBuffer, AudioError> {
        if !start_sec.is_finite() || !end_sec.is_finite() {
            return Err(AudioError::InvalidParam(
                "slice endpoints must be finite".to_string(),
            ));
        }
        if start_sec < 0.0 || end_sec < 0.0 {
            return Err(AudioError::InvalidParam(
                "slice endpoints must be >= 0".to_string(),
            ));
        }
        if start_sec > end_sec {
            return Err(AudioError::InvalidParam(format!(
                "start_sec ({}) must be <= end_sec ({})",
                start_sec, end_sec
            )));
        }
        let total = self.duration_secs();
        let start = start_sec.min(total);
        let end = end_sec.min(total);

        let ch = self.channels as usize;
        let start_frame = (start * self.sample_rate as f64) as usize;
        let end_frame = ((end * self.sample_rate as f64) as usize).max(start_frame);

        let start_idx = start_frame.checked_mul(ch).unwrap_or(0);
        let mut end_idx = end_frame.checked_mul(ch).unwrap_or(self.samples.len());
        end_idx = end_idx.min(self.samples.len());
        let start_idx = start_idx.min(end_idx);

        let sliced = self.samples[start_idx..end_idx].to_vec();
        AudioBuffer::from_samples(sliced, self.sample_rate, self.channels)
    }

    /// Compute a snapshot of buffer statistics (peak, RMS, frames,
    /// duration). Used by tests + insta snapshots; not part of the
    /// core audio API surface but useful for diagnostics.
    pub fn summarize(&self) -> AudioSummary {
        let peak = self.samples.iter().map(|s| s.abs()).fold(0.0_f32, f32::max);
        let sum_sq: f64 = self.samples.iter().map(|s| (*s as f64) * (*s as f64)).sum();
        let n = self.samples.len().max(1);
        let rms = (sum_sq / n as f64) as f32;
        AudioSummary {
            frames: self.frames(),
            channels: self.channels,
            sample_rate: self.sample_rate,
            duration_secs: self.duration_secs(),
            peak,
            rms,
        }
    }
}

impl fmt::Display for AudioBuffer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "AudioBuffer({}ch, {} Hz, {:.3}s, {} frames)",
            self.channels,
            self.sample_rate,
            self.duration_secs(),
            self.frames()
        )
    }
}

/// Compact statistics snapshot used by tests + snapshots (NOT the
/// `Display` impl — `Display` for `AudioBuffer` itself is one-line).
#[derive(Debug, Clone, PartialEq)]
pub struct AudioSummary {
    pub frames: usize,
    pub channels: u16,
    pub sample_rate: u32,
    pub duration_secs: f64,
    pub peak: f32,
    pub rms: f32,
}

impl fmt::Display for AudioSummary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "AudioSummary({}ch, {} Hz, {:.3}s, peak={:.4}, rms={:.4})",
            self.channels, self.sample_rate, self.duration_secs, self.peak, self.rms
        )
    }
}

#[cfg(test)]
mod smoke_tests {
    use super::*;

    #[test]
    fn empty_buffer_construction_round_trip() {
        let buf = AudioBuffer::from_samples(Vec::new(), 44100, 2).expect("empty ok");
        assert_eq!(buf.samples().len(), 0);
        assert_eq!(buf.frames(), 0);
        assert_eq!(buf.duration_secs(), 0.0);
    }

    #[test]
    fn rejects_zero_channels() {
        let err = AudioBuffer::from_samples(Vec::new(), 44100, 0).unwrap_err();
        assert!(matches!(err, AudioError::InvalidParam(_)));
    }

    #[test]
    fn rejects_zero_sample_rate() {
        let err = AudioBuffer::from_samples(Vec::new(), 0, 2).unwrap_err();
        assert!(matches!(err, AudioError::InvalidParam(_)));
    }

    #[test]
    fn rejects_misaligned_samples() {
        let err = AudioBuffer::from_samples(vec![0.1, 0.2, 0.3], 44100, 2).unwrap_err();
        assert!(matches!(err, AudioError::InvalidParam(_)));
    }
}
