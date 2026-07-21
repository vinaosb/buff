# buff-audio

AudioBuffer + sample ops for the Buff language. v1.13 frameworks wave 2
wrapper crate (task T10). Wraps `hound` (WAV) + `symphonia` (MP3/FLAC/Vorbis)
via the FFI guide.

## STRUCTURE

```
buff-audio/
├── Cargo.toml        # workspace deps (hound, symphonia, thiserror) + dev-deps
├── README.md         # public-facing crate overview
├── AGENTS.md         # this file
├── src/
│   └── lib.rs        # the entire crate (~600 LOC, single file)
├── examples/
│   ├── audio/                 # .buff forward-decls (round_trip / mix / generate)
│   ├── round_trip.rs          # synthesize → save → reload → verify
│   ├── load_and_inspect.rs    # from_path + accessors + summarize
│   ├── generate.rs            # programmatic tone generator
│   ├── generate_test_wav.rs   # helper that writes a test.wav fixture
│   ├── amplify_and_mix.rs     # amplify + mix + slice pipeline
│   └── mix.rs                 # minimal mix example
└── tests/
    ├── api.rs                 # public API + accessors
    ├── ops.rs                 # amplify / normalize / mix / slice
    ├── snapshots.rs           # insta snapshot driver
    └── snapshots/             # insta .snap files (10 snapshots)
```

## WHERE TO LOOK

| Task | File |
|---|---|
| Add a new instance op | `src/lib.rs` `impl AudioBuffer { ... }` + a test in `tests/ops.rs` |
| Add a new codec | `src/lib.rs` `from_symphonia` arm + workspace dep |
| Add an extern "C" surface | Out of scope for T10 (deferred to v1.15+ Wave 3 production wrappers). The T10 crate is safe-Rust-only — the codegen lowering uses the safe `buff_audio::AudioBuffer::*` API directly (no FFI indirection). |
| Tune sample conversion | `src/lib.rs` `from_wav` int-bits-per-sample match + `copy_to_vec_interleaved` |
| Add a Buff-visible assoc fn | `crates/buff-lang-types/src/prelude_types.rs` `PreludeType::Audio` registry |
| Add codegen lowering | `crates/buff-lang-codegen-rust/src/rust_codegen.rs` `lower_prelude_type_assoc_fn` / `lower_prelude_type_instance_fn` arms |

## CONVENTIONS (this crate only)

- **Single-file crate** — `src/lib.rs` is the entire implementation. No
  submodules. If split becomes necessary, follow the buff-registry pattern
  (`error.rs` + `storage.rs` + `handlers.rs`).
- **No `unwrap`/`expect`/`panic!`/`todo!`/`unimplemented!`** in non-test
  code (workspace hard rule from AGENTS.md).
- **`catch_unwind` on I/O boundary** — `from_path` / `save` wrap their
  bodies in `catch_unwind` per FFI guide R6 (a panic inside the codec
  becomes `Err(AudioError::Decode)` / `Err(AudioError::Encode)` instead
  of process abort). No `extern "C"` surface exists in this crate — the
  catch_unwind is defensive against codec panics, not FFI requirements.
- **32-bit float WAV** is the canonical save format (lossless round-trip
  for our f32 samples). WAV int 8/16/24/32 is supported on decode.
- **Interleaved layout** — frame-major `[L0, R0, L1, R1, ...]` for stereo.
  Symphonia returns planar; `copy_to_vec_interleaved::<f32>(&mut out)`
  handles the planar-to-interleaved conversion in one call.
- **Clamp on decode** — samples outside `[-1.0, 1.0]` are clamped post-
  conversion. Defensive measure against encoder quirks (notably MP3 decode
  can produce values marginally over 1.0 due to inter-sample peaks).
- **`AudioBuffer` impls Default** as an empty 44100Hz mono buffer (added
  in the T10 finish commit so the codegen lowering can use
  `unwrap_or_default()` panic-free on `Result<AudioBuffer, AudioError>`
  returning methods — matches the DataFrame / Image precedent).

## RELATIONSHIP TO OTHER CRATES

| Crate | Relationship |
|---|---|
| `buff-lang-types` | `Type::Audio` variant + `PreludeType::Audio` registry entry. The buff_type() mapping returns `Type::Audio`; the prelude_type_lookup matches `"AudioBuffer"`. Assoc fns `FromPath` + `FromSamples` registered. Instance methods `Samples` / `SampleRate` / `Channels` / `Frames` / `DurationSecs` / `Amplify` / `Normalize` / `Mix` / `Slice` / `Summarize` + shared `Save` registered. |
| `buff-lang-codegen-rust` | `Type::Audio => "buff_audio::AudioBuffer"` arm in `buff_type_to_syn`. `(Audio, FromPath)` / `(Audio, FromSamples)` arms in `lower_prelude_type_assoc_fn`. 10 instance-method arms + shared `Save` arm in `lower_prelude_type_instance_fn`. `program_uses_namespace("AudioBuffer")` walker records `buff-audio` + `hound` + `symphonia` in `extern_crates`. All shipped in the T10 finish commit. |
| `buff-lang-ffi-guide` | Authoritative source for the six FFI rules this crate's public surface follows. The crate has NO `extern "C"` fns (the v1.13 wave-2 MVP wrapper exposes only safe Rust); the FFI guide's R1-R6 rules still apply to the safe API as panic-safety / ownership / lifetime / thread-safety invariants. A production C-ABI surface is deferred to v1.15+ (Wave 3 wrappers). |
| `hound` | Pure-Rust WAV reader/writer (3.x). Used for WAV decode + the only encode format shipped in T10. |
| `symphonia` | Pure-Rust audio decoder (0.6). Used for MP3 / FLAC / Vorbis / non-WAV decode. Feature flags `mp3`, `flac`, `wav`, `vorbis`, `pcm` enabled. |

## NOTES

- **`rodio` was rejected** — the T10 task spec lists `rodio` as the primary
  candidate, but `rodio` pulls in `cpal` for audio output, and `cpal`
  requires native C deps via cc-rs on Linux (alsa-sys) and macOS
  (coreaudio-sys). That breaks the "no C library, no Docker" hard rule AND
  fails on this Windows MSVC vcruntime.h host. `symphonia` is the
  canonical pure-Rust decoder-only alternative (the spec explicitly lists
  it as an alternative). Documented in the workspace Cargo.toml.
- **Sample conversion via `GenericAudioBufferRef::copy_to_vec_interleaved::<f32>`**:
  symphonia 0.6 ships this method on `GenericAudioBufferRef` directly. The
  `ConvertibleSample` trait auto-impls for f32 (it has `FromSample` for
  every source format). One-line lowering — no per-variant match needed.
- **NO `AUDIO_STORE` static** — the v1.13 wave-2 MVP wrapper has no
  `extern "C"` surface, so there's no per-process handle table. The
  codegen lowering uses the safe Rust API directly (`buff_audio::
  AudioBuffer::*`). A handle-table + extern C surface is deferred to
  v1.15+ Wave 3 production wrappers (matches the FFI guide's two-tier
  MVP-vs-production wrapper distinction).
- **`save` is WAV-only** for T10. Encoding to MP3 / FLAC / Vorbis is heavy
  (lame / flac encoders aren't pure-Rust or are unstable) — the WAV
  round-trip covers all three MVP acceptance scenarios.
- **MSVC host limitation** — this Windows host fails to link test binaries
  (`LNK1104: cannot open file 'msvcrt.lib'`). `cargo check -p buff-audio`
  succeeds (compiles cleanly); `cargo test -p buff-audio` requires Linux
  CI to actually run. Documented in the task spec's CONTEXT block.
