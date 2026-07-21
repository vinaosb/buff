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
│   └── audio/        # example .rs files + a generated test.wav fixture
└── tests/
    ├── api.rs        # public API + accessors
    ├── ops.rs        # amplify / normalize / mix / slice
    ├── io.rs         # WAV round-trip via hound
    ├── symphonia.rs  # MP3/FLAC decode (smoke)
    ├── ffi.rs        # extern "C" surface
    └── snapshots/    # insta snapshot files
```

## WHERE TO LOOK

| Task | File |
|---|---|
| Add a new instance op | `src/lib.rs` `impl AudioBuffer { ... }` + a test in `tests/ops.rs` |
| Add a new codec | `src/lib.rs` `from_symphonia` arm + workspace dep |
| Add a new extern fn | `src/lib.rs` `extern "C" fn buff_audio_*` + FFI guide R1-R6 |
| Tune sample conversion | `src/lib.rs` `from_wav` int-bits-per-sample match + `interleave_ref` |
| Add a Buff-visible assoc fn | `crates/buff-lang-types/src/prelude_types.rs` `PreludeType::Audio` registry |
| Add codegen lowering | `crates/buff-lang-codegen-rust/src/rust_codegen.rs` `Type::Audio` arm |

## CONVENTIONS (this crate only)

- **Single-file crate** — `src/lib.rs` is the entire implementation. No
  submodules. If split becomes necessary, follow the buff-registry pattern
  (`error.rs` + `storage.rs` + `handlers.rs`).
- **No `unwrap`/`expect`/`panic!`/`todo!`/`unimplemented!`** in non-test
  code (workspace hard rule from AGENTS.md).
- **Every extern fn body wrapped in `catch_unwind`** (FFI guide R6).
- **32-bit float WAV** is the canonical save format (lossless round-trip
  for our f32 samples). WAV int 8/16/24/32 is supported on decode.
- **Interleaved layout** — frame-major `[L0, R0, L1, R1, ...]` for stereo.
  Symphonia returns planar; `copy_to_vec_interleaved::<f32>(&mut out)`
  handles the planar-to-interleaved conversion in one call.
- **Clamp on decode** — samples outside `[-1.0, 1.0]` are clamped post-
  conversion. Defensive measure against encoder quirks (notably MP3 decode
  can produce values marginally over 1.0 due to inter-sample peaks).

## RELATIONSHIP TO OTHER CRATES

| Crate | Relationship |
|---|---|
| `buff-lang-types` | `Type::Audio` variant + `PreludeType::Audio` registry entry. The buff_type() mapping returns `Type::Audio`; the prelude_type_lookup matches `"AudioBuffer"`. |
| `buff-lang-codegen-rust` | Future codegen lowering arm for `Type::Audio` (NOT shipped in T10 — sibling codegen task). The `extern_crates` BTreeSet will record `"buff_audio"` when a Buff program uses `AudioBuffer`. |
| `buff-lang-ffi-guide` | Authoritative source for the six FFI rules every `buff_audio_*` extern fn follows. |
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
- **The static `AUDIO_STORE` is per-process** — a `Mutex<Option<HashMap<u64,
  AudioBuffer>>>`. Mirrors the FFI guide's regex example (Example 2:
  stateful struct wrapper). Drop is explicit via `buff_audio_drop(handle)`;
  unknown handle ids are a no-op (idempotent).
- **`save` is WAV-only** for T10. Encoding to MP3 / FLAC / Vorbis is heavy
  (lame / flac encoders aren't pure-Rust or are unstable) — the WAV
  round-trip covers all three MVP acceptance scenarios.
- **MSVC host limitation** — this Windows host fails to link test binaries
  (`LNK1104: cannot open file 'msvcrt.lib'`). `cargo check -p buff-audio`
  succeeds (compiles cleanly); `cargo test -p buff-audio` requires Linux
  CI to actually run. Documented in the task spec's CONTEXT block.
