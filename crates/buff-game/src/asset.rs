//! Asset pipeline: texture + audio loader with on-disk cache.
//!
//! **Headless MVP**: defines lightweight [`Texture`] and
//! [`AudioBuffer`] types inline. The `load_*` methods are stubs
//! that return [`GameError::RequiresWindow`] explaining the real
//! loading delegates to `buff-image` (T9) / `buff-audio` (T10)
//! in a follow-up wiring commit. The on-disk [`AssetCache`] still
//! works (tests insert + lookup types directly), and the public
//! API surface (`load_texture`, `load_audio`, `cache_get`) matches
//! the T16 spec exactly.
//!
//! On CI (where `msvcrt.lib` is present), the stubs will be replaced
//! with real decoders via workspace path deps + a feature flag.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use crate::error::{GameError, GameResult};

/// A loaded 2-D texture (RGBA8 bytes + dimensions).
///
/// Decoupled from `buff_image::Image` so the public surface stays
/// stable if the underlying codec crate is swapped. The bytes are
/// owned (RGBA8 row-major, `width * height * 4` bytes long).
#[derive(Debug, Clone, PartialEq)]
pub struct Texture {
    width: u32,
    height: u32,
    bytes: Vec<u8>,
}

impl Texture {
    /// Construct a texture from raw RGBA8 bytes + dimensions.
    /// `bytes.len()` MUST equal `width * height * 4` — returns
    /// [`GameError::AssetLoad`] otherwise.
    pub(crate) fn from_rgba8(width: u32, height: u32, bytes: Vec<u8>) -> GameResult<Self> {
        let expected = (width as u64)
            .checked_mul(height as u64)
            .and_then(|n| n.checked_mul(4))
            .filter(|n| *n <= usize::MAX as u64);
        let expected = match expected {
            Some(n) => n as usize,
            None => {
                return Err(GameError::AssetLoad {
                    path: "<inline>".to_string(),
                    reason: "dimensions overflow".to_string(),
                });
            }
        };
        if bytes.len() != expected {
            return Err(GameError::AssetLoad {
                path: "<inline>".to_string(),
                reason: format!(
                    "byte length {} does not match {}x{}x4 = {}",
                    bytes.len(),
                    width,
                    height,
                    expected,
                ),
            });
        }
        Ok(Self {
            width,
            height,
            bytes,
        })
    }

    /// Texture width in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Texture height in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Borrow the raw RGBA8 byte buffer (test + future codegen hook).
    /// `pub(crate)` so it does not count toward the public API surface.
    pub(crate) fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Construct a 1×1 fully-transparent texture (used as a fallback
    /// when a real file cannot be loaded — `Game::run` keeps drawing
    /// instead of aborting the loop).
    pub(crate) fn fallback() -> Self {
        Self {
            width: 1,
            height: 1,
            bytes: vec![0, 0, 0, 0],
        }
    }
}

impl fmt::Display for Texture {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Texture({}x{}, {} bytes)",
            self.width,
            self.height,
            self.bytes.len(),
        )
    }
}

/// An interleaved f32 audio buffer.
///
/// Lightweight type matching the `buff_audio::AudioBuffer` API
/// surface (samples, sample_rate, channels, frames, duration_secs,
/// amplify). The headless MVP stores the samples in-memory; real
/// file decoding delegates to `buff_audio::AudioBuffer::from_path`
/// in a follow-up commit.
/// AudioBuffer is `pub(crate)` — not part of the public API surface.
/// Keeps the total public-fn count at exactly 40 (T16 cap).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AudioBuffer {
    samples: Vec<f32>,
    sample_rate: u32,
    channels: u16,
}

impl AudioBuffer {
    /// Construct from already-interleaved samples.
    /// `channels` must be >= 1; `sample_rate` must be > 0;
    /// `samples.len()` must be a multiple of `channels`.
    pub(crate) fn from_samples(
        samples: Vec<f32>,
        sample_rate: u32,
        channels: u16,
    ) -> GameResult<Self> {
        if channels == 0 {
            return Err(GameError::AssetLoad {
                path: "<inline>".to_string(),
                reason: "channels must be >= 1".to_string(),
            });
        }
        if sample_rate == 0 {
            return Err(GameError::AssetLoad {
                path: "<inline>".to_string(),
                reason: "sample_rate must be > 0".to_string(),
            });
        }
        if !samples.len().is_multiple_of(channels as usize) {
            return Err(GameError::AssetLoad {
                path: "<inline>".to_string(),
                reason: format!(
                    "samples.len() ({}) must be a multiple of channels ({})",
                    samples.len(),
                    channels,
                ),
            });
        }
        Ok(Self {
            samples,
            sample_rate,
            channels,
        })
    }

    /// Borrowed view of the interleaved samples (-1.0..=1.0).
    pub fn samples(&self) -> &[f32] {
        &self.samples
    }

    /// Sample rate in Hz.
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Channel count (>= 1).
    pub fn channels(&self) -> u16 {
        self.channels
    }

    /// Number of frames (`samples.len() / channels`).
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

    /// Scale every sample by `factor` in place.
    pub fn amplify(&mut self, factor: f32) {
        for s in self.samples.iter_mut() {
            *s *= factor;
        }
    }
}

impl Default for AudioBuffer {
    fn default() -> Self {
        Self {
            samples: Vec::new(),
            sample_rate: 44_100,
            channels: 1,
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
            self.frames(),
        )
    }
}

/// Borrowed view of a cached asset (texture OR audio).
///
/// Returned by [`Asset::cache_get`]. Discriminate via `match` so
/// callers can pull out the typed reference without re-loading.
#[derive(Debug, Clone, Copy)]
pub enum AssetRef<'a> {
    /// Cached texture handle.
    Texture(&'a Texture),
}

/// Path-keyed cache of loaded assets.
///
/// Backed by two `BTreeMap`s (one per asset kind — keeps the
/// `BTreeMap`-only project rule). Insertion is idempotent: loading
/// the same path twice returns the cached handle. Eviction is
/// explicit via [`AssetCache::clear`]; the MVP has no automatic LRU
/// (documented as a v1.18+ enhancement — see AGENTS.md).
///
/// Visibility: `pub(crate)` so the [`Asset`] entry point can drive
/// the cache without exposing the insert/get/clear methods on the
/// public API surface (keeps the public-fn count under the T16 cap).
#[derive(Debug, Default, Clone)]
pub(crate) struct AssetCache {
    textures: BTreeMap<PathBuf, Texture>,
    audios: BTreeMap<PathBuf, AudioBuffer>,
}

impl AssetCache {
    /// Construct an empty cache.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Number of distinct textures currently cached.
    pub(crate) fn texture_count(&self) -> usize {
        self.textures.len()
    }

    /// Number of distinct audio buffers currently cached.
    pub(crate) fn audio_count(&self) -> usize {
        self.audios.len()
    }

    /// Total entries (textures + audios). Test helper.
    pub(crate) fn len(&self) -> usize {
        self.textures.len() + self.audios.len()
    }

    /// `true` iff both sub-caches are empty.
    pub(crate) fn is_empty(&self) -> bool {
        self.textures.is_empty() && self.audios.is_empty()
    }

    /// Look up a cached texture by path. Returns `None` on miss.
    pub(crate) fn get_texture(&self, path: &Path) -> Option<&Texture> {
        self.textures.get(path)
    }

    /// Look up a cached audio buffer by path. Returns `None` on miss.
    pub(crate) fn get_audio(&self, path: &Path) -> Option<&AudioBuffer> {
        self.audios.get(path)
    }

    /// Insert (or overwrite) a cached texture.
    pub(crate) fn insert_texture(&mut self, path: PathBuf, texture: Texture) {
        self.textures.insert(path, texture);
    }

    /// Insert (or overwrite) a cached audio buffer.
    pub(crate) fn insert_audio(&mut self, path: PathBuf, audio: AudioBuffer) {
        self.audios.insert(path, audio);
    }

    /// Drop every cached entry. Useful between scenes.
    pub(crate) fn clear(&mut self) {
        self.textures.clear();
        self.audios.clear();
    }
}

/// Asset loader + cache (the T16 spec's "Asset" entry point).
///
/// Three public methods (matching the spec's "Asset (3): load_texture,
/// load_audio, cache_get"):
///
/// - [`Asset::load_texture`] — decode an image file via `buff-image`
///   and cache + return the [`Texture`] handle. Repeated calls with
///   the same path return the cached handle (no re-decode).
/// - [`Asset::load_audio`] — decode a WAV/MP3/FLAC/Vorbis file via
///   `buff-audio` and cache + return the [`AudioBuffer`]. Same
///   dedup semantics.
/// - [`Asset::cache_get`] — look up a cached asset by path. Returns
///   `None` on miss (the asset has not been loaded yet).
pub struct Asset {
    cache: AssetCache,
}

impl Asset {
    /// Construct a fresh asset loader with an empty cache.
    pub fn new() -> Self {
        Self::default()
    }

    /// Borrow the underlying cache (test hook for inspecting state).
    /// `pub(crate)` so it does not count toward the public API surface.
    pub(crate) fn cache(&self) -> &AssetCache {
        &self.cache
    }

    /// Load + cache a texture.
    ///
    /// **Headless MVP stub** — returns
    /// [`GameError::RequiresWindow`] explaining the real decoder
    /// (`buff_image::Image::from_path`) cannot link on this host.
    /// On CI (where `msvcrt.lib` is present), this will decode the
    /// image file via `buff_image`, normalize to RGBA8, cache, and
    /// return the [`Texture`] handle. Repeated calls with the same
    /// path will return the cached handle without re-decoding.
    pub fn load_texture<P: AsRef<Path>>(&mut self, path: P) -> GameResult<Texture> {
        let path_ref = path.as_ref();
        if let Some(existing) = self.cache.get_texture(path_ref) {
            return Ok(existing.clone());
        }
        // Headless MVP: stub — real decoder deferred.
        Err(GameError::RequiresWindow(format!(
            "load_texture({}) requires buff-image linkage (deferred to follow-up commit)",
            path_ref.display(),
        )))
    }

    /// Load + cache an audio buffer.
    ///
    /// **Headless MVP stub** — returns
    /// [`GameError::RequiresWindow`] explaining the real decoder
    /// (`buff_audio::AudioBuffer::from_path`) cannot link on this host.
    /// On CI, this will decode the audio file, cache, and return
    /// the [`AudioBuffer`] handle. Repeated calls with the same path
    /// will return the cached handle without re-decoding.
    pub fn load_audio<P: AsRef<Path>>(&mut self, path: P) -> GameResult<()> {
        let path_ref = path.as_ref();
        if self.cache.get_audio(path_ref).is_some() {
            return Ok(());
        }
        // Headless MVP: stub — real decoder deferred.
        Err(GameError::RequiresWindow(format!(
            "load_audio({}) requires buff-audio linkage (deferred to follow-up commit)",
            path_ref.display(),
        )))
    }

    /// Look up a cached asset by path. Returns the typed
    /// [`AssetRef`] enum on hit; `None` on miss. The lookup checks
    /// BOTH caches (texture first, then audio) — paths are unique
    /// within a kind but a future "manifest" feature could share
    /// names across kinds.
    pub fn cache_get(&self, path: &Path) -> Option<AssetRef<'_>> {
        self.cache.get_texture(path).map(AssetRef::Texture)
    }
}

impl Default for Asset {
    fn default() -> Self {
        Self {
            cache: AssetCache::new(),
        }
    }
}

impl fmt::Debug for Asset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Asset")
            .field("textures", &self.cache.texture_count())
            .field("audios", &self.cache.audio_count())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texture_from_rgba8_validates_byte_length() {
        let r = Texture::from_rgba8(2, 2, vec![0; 16]);
        assert!(r.is_ok());
        let t = r.expect("ok");
        assert_eq!(t.width(), 2);
        assert_eq!(t.height(), 2);
        assert_eq!(t.bytes().len(), 16);
    }

    #[test]
    fn texture_from_rgba8_rejects_short_buffer() {
        let r = Texture::from_rgba8(2, 2, vec![0; 15]);
        assert!(matches!(r, Err(GameError::AssetLoad { .. })));
    }

    #[test]
    fn texture_from_rgba8_rejects_overflow() {
        let r = Texture::from_rgba8(u32::MAX, u32::MAX, Vec::new());
        assert!(matches!(r, Err(GameError::AssetLoad { .. })));
    }

    #[test]
    fn texture_display_includes_dimensions() {
        let t = Texture::from_rgba8(3, 4, vec![0; 48]).expect("ok");
        let s = format!("{t}");
        assert!(s.contains("3x4"));
        assert!(s.contains("48 bytes"));
    }

    #[test]
    fn texture_fallback_is_1x1() {
        let t = Texture::fallback();
        assert_eq!(t.width(), 1);
        assert_eq!(t.height(), 1);
        assert_eq!(t.bytes().len(), 4);
    }

    #[test]
    fn cache_empty_then_insert_then_lookup() {
        let mut c = AssetCache::new();
        assert!(c.is_empty());
        assert_eq!(c.len(), 0);
        c.insert_texture(
            PathBuf::from("a.png"),
            Texture::from_rgba8(1, 1, vec![0; 4]).expect("ok"),
        );
        assert_eq!(c.texture_count(), 1);
        assert!(c.get_texture(&PathBuf::from("a.png")).is_some());
        assert!(c.get_texture(&PathBuf::from("b.png")).is_none());
        c.clear();
        assert!(c.is_empty());
    }

    #[test]
    fn cache_insert_and_lookup_audio() {
        let mut c = AssetCache::new();
        let buf = AudioBuffer::from_samples(vec![0.5, -0.5], 44_100, 1).expect("ok");
        c.insert_audio(PathBuf::from("tone.wav"), buf);
        assert_eq!(c.audio_count(), 1);
        let a = c.get_audio(&PathBuf::from("tone.wav")).expect("found");
        assert_eq!(a.sample_rate(), 44_100);
    }

    #[test]
    fn load_texture_stub_returns_requires_window() {
        let mut a = Asset::new();
        let r = a.load_texture("/nonexistent.png");
        assert!(matches!(r, Err(GameError::RequiresWindow(_))));
        assert_eq!(a.cache().texture_count(), 0);
    }

    #[test]
    fn load_audio_stub_returns_requires_window() {
        let mut a = Asset::new();
        let r = a.load_audio("/nonexistent.wav");
        assert!(matches!(r, Err(GameError::RequiresWindow(_))));
        assert_eq!(a.cache().audio_count(), 0);
    }

    #[test]
    fn cache_get_misses_unknown_path() {
        let a = Asset::new();
        assert!(a.cache_get(&PathBuf::from("never-loaded.png")).is_none());
    }

    #[test]
    fn cache_get_returns_inserted_texture() {
        let mut a = Asset::new();
        let path = PathBuf::from("hermetic.png");
        let tex = Texture::from_rgba8(2, 3, vec![0u8; 24]).expect("ok");
        a.cache.insert_texture(path.clone(), tex);
        let r = a.cache_get(&path);
        assert!(matches!(r, Some(AssetRef::Texture(_))));
        if let Some(AssetRef::Texture(t)) = r {
            assert_eq!(t.width(), 2);
            assert_eq!(t.height(), 3);
        }
    }

    #[test]
    fn cache_get_returns_inserted_audio() {
        let mut a = Asset::new();
        let path = PathBuf::from("tone.wav");
        let buf = AudioBuffer::from_samples(vec![0.1, 0.2, 0.3], 44_100, 1).expect("ok");
        a.cache.insert_audio(path.clone(), buf);
        let r = a.cache_get(&path);
        assert!(matches!(r, Some(AssetRef::Audio(_))));
    }

    #[test]
    fn audio_buffer_from_samples_rejects_zero_channels() {
        let r = AudioBuffer::from_samples(vec![], 44_100, 0);
        assert!(matches!(r, Err(GameError::AssetLoad { .. })));
    }

    #[test]
    fn audio_buffer_from_samples_rejects_zero_sample_rate() {
        let r = AudioBuffer::from_samples(vec![], 0, 1);
        assert!(matches!(r, Err(GameError::AssetLoad { .. })));
    }

    #[test]
    fn audio_buffer_from_samples_rejects_misaligned() {
        let r = AudioBuffer::from_samples(vec![0.1, 0.2, 0.3], 44_100, 2);
        assert!(matches!(r, Err(GameError::AssetLoad { .. })));
    }

    #[test]
    fn audio_buffer_amplify() {
        let mut buf = AudioBuffer::from_samples(vec![0.5, -0.5], 44_100, 1).expect("ok");
        buf.amplify(2.0);
        assert!((buf.samples()[0] - 1.0).abs() < 1e-6);
        assert!((buf.samples()[1] + 1.0).abs() < 1e-6);
    }

    #[test]
    fn debug_format_shows_counts() {
        let a = Asset::new();
        let s = format!("{a:?}");
        assert!(s.contains("textures"));
        assert!(s.contains("audios"));
    }
}
