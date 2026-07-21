# buff-audio

AudioBuffer + sample ops for the Buff language — the v1.13 frameworks wave 2
wrapper crate (task T10 of `.sisyphus/plans/buff-v1x-frameworks.md`).

Wraps two pure-Rust audio crates via the FFI guide:
- **`hound`** 3.x — WAV read + write (the canonical pure-Rust WAV codec).
- **`symphonia`** 0.6 — MP3 / FLAC / Vorbis / WAV decode (pure-Rust, no
  native C deps via cc-rs; matches the workspace hard rule).

CPU-only MVP per Metis G7 (NO GPU dispatch). Real-time playback is
explicitly out of scope (deferred to v1.18+); synthesis is delegated to
`buff-dsp` (T11).

## Public API surface

| Type / Fn | Description |
|---|---|
| [`AudioBuffer`] | Owned interleaved f32 samples + sample_rate + channels. |
| [`AudioBuffer::from_samples`] | Construct from raw `Vec<f32>` (programmatic). |
| [`AudioBuffer::from_path`] | Decode any WAV/MP3/FLAC/Vorbis file. |
| `samples` / `sample_rate` / `channels` / `frames` / `duration_secs` | Read-only accessors. |
| [`AudioBuffer::save`] | Encode to a 32-bit float WAV file. |
| [`AudioBuffer::amplify`] | Scale every sample by `factor` in place. |
| [`AudioBuffer::normalize`] | Peak-normalize to a target amplitude. |
| [`AudioBuffer::mix`] | Sample-wise add another buffer. |
| [`AudioBuffer::slice`] | Return a sub-region as a new buffer. |
| [`AudioBuffer::summarize`] | Compute peak / RMS / frames / duration stats. |
| [`AudioError`] | thiserror-derived error type (Io / Decode / Encode / InvalidParam). |
| [`AudioSummary`] | Plain-statistics snapshot for tests + snapshots. |

The v1.13 wave-2 MVP wrapper is **safe-Rust-only** — no `extern "C"`
surface. The Buff codegen layer lowers `AudioBuffer.*` calls directly
to the safe `buff_audio::AudioBuffer::*` Rust API (no FFI indirection,
no per-process handle table). A production C-ABI surface is deferred
to v1.15+ Wave 3 wrappers per the FFI guide's two-tier MVP-vs-production
distinction.

## FFI safety

Every public function follows the six hard rules in
`crates/buff-lang-ffi-guide/GUIDE.md`:
- **R1** no raw pointers (samples are `Vec<f32>` / `&[f32]`).
- **R2** Rust owns all heap memory (returns owned `AudioBuffer` / `Vec<f32>`).
- **R3** errors mapped to `AudioError` (thiserror + Display).
- **R4** `AudioBuffer` is `Send + 'static` (owned `Vec<f32>` + two `Copy` fields).
- **R5** no public lifetimes in signatures.
- **R6** `catch_unwind` wraps the I/O boundary (`from_path` / `save`)
  so a panic inside the codec library becomes a structured
  `AudioError::{Decode, Encode}` instead of process abort. There is no
  `extern "C"` surface in this crate — the catch_unwind is defensive
  against codec panics, not an FFI requirement.

## Out of scope (deferred)

- Real-time playback / streaming (deferred to v1.18+ per T10 spec).
- Synthesis (sine/square/noise generators) — that's `buff-dsp` T11.
- GPU acceleration (CPU-only per Metis G7).
- Encoding to non-WAV formats (FLAC/MP3 encoding is heavy; WAV round-trip
  covers the MVP acceptance scenarios).

## License

Dual-licensed under [MIT](../../LICENSE) or [Apache-2.0](../../LICENSE),
matching the rest of the Buff workspace.
