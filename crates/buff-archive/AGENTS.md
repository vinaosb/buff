# buff-archive

Zip / Tar / Gz / Zstd compression for the Buff language. Pure-Rust MVP wrapping four mature Rust crates via a safe FFI boundary per the [T4 FFI guide](../buff-lang-ffi-guide/GUIDE.md).

**Status: experimental** (T39 v1.17 frameworks wave 6).

## STRUCTURE

```
buff-archive/
├── Cargo.toml                       # zip + tar + flate2 + ruzstd + thiserror + insta deps
├── src/
│   ├── lib.rs                       # Format enum + Archive namespace + 4 entry points (~490 LOC)
│   └── error.rs                     # ArchiveError enum (~110 LOC)
├── examples/
│   ├── archive_zip.rs               # roundtrip dir via ZIP
│   ├── archive_tar.rs               # roundtrip dir via TAR
│   ├── archive_gz.rs                # roundtrip dir via .tar.gz
│   ├── archive_zstd.rs              # roundtrip dir via .tar.zst
│   └── archive/
│       ├── archive_zip.buff         # Buff-side forward-decls (matches .rs)
│       ├── archive_tar.buff
│       ├── archive_gz.buff
│       └── archive_zstd.buff
└── tests/
    └── core.rs                      # 16 unit tests + 5 insta snapshots (~400 LOC)
```

Total: ~1000 LOC (well under the 2000 LOC T39 cap).

## WHERE TO LOOK

| Task | File |
|---|---|
| Add a new format | `src/lib.rs::Format` (add variant + 3 match arms in `extension` / `from_extension` / `is_multifile`) + `src/error.rs` (add error variant if needed) + `tests/core.rs` (roundtrip test) |
| Change the dir-walk order | `src/lib.rs::collect_dir_entries` / `visit_dir` |
| Tune the gzip compression level | `src/lib.rs::write_tar` (Gz arm) + `compress_bytes_inner` (Gz arm) — both hard-coded to `flate2::Compression::default()` (= 6) |
| Tune the zstd compression level | `src/lib.rs::write_tar` (Zstd arm) + `compress_bytes_inner` (Zstd arm) — both hard-coded to `ruzstd::encoding::CompressionLevel::Fastest` |
| Wire a Buff-side method to codegen | `crates/buff-lang-types/src/prelude_types.rs` (PreludeAssocFn + `assoc_fn_return_type`) + `crates/buff-lang-codegen-rust/src/rust_codegen.rs::lower_prelude_type_assoc_fn` |

## PUBLIC API (7 functions, ≤15 cap)

### `Format` enum (4 variants + 4 functions)
- Variants: `Zip`, `Tar`, `Gz`, `Zstd`
- Constructors: `from_extension(&str)` (case-insensitive; `zip`/`tar`/`gz`/`gzip`/`zst`/`zstd` accepted)
- Accessors: `extension(&self)` (canonical ext without dot), `from_path(&Path)` (handles `.tar.gz` / `.tar.zst` compounds)
- Predicates: `is_multifile(&self)` (true for `Zip`/`Tar`, false for `Gz`/`Zstd`)

### `Archive` namespace (4 functions, namespace-only — never instantiated)
- `Archive::compress_dir(input_dir, output_path, format)` — pack dir → archive file (`.zip`/`.tar`/`.tar.gz`/`.tar.zst`). **Rust API**: 3-arg (caller passes the `Format` enum). **Buff surface (codegen-lowered)**: 2-arg — the third `format` arg is auto-detected from the `output_path` extension via `Format::from_path()` (matches the cross-language convention of `tar -czf x.tar.gz src/`).
- `Archive::extract(archive_path, output_dir)` — auto-detected format from extension
- `Archive::compress_bytes(bytes, format)` — single-stream compress (Gz / Zstd only)
- `Archive::decompress_bytes(bytes, format)` — single-stream decompress (Gz / Zstd only)

## CONVENTIONS

- **Pure-Rust only**: `zip` (deflate feature only — disables C libzstd transitively), `tar`, `flate2` (default `miniz_oxide` backend), `ruzstd` (NOT the canonical `zstd` crate which wraps C libzstd). Matches the "no C library, no Docker" hard rule from AGENTS.md.
- **`ruzstd` deviation from T39 spec**: the task spec listed `zstd` as a backend. Verification (`crates/buff-lang-ffi-guide/GUIDE.md` librarian report) confirmed the canonical `zstd` crate invokes `cc::Build::compile()` on Facebook's zstd C source via `zstd-sys`. That violates the buff workspace's hard "no cc-rs" rule. We pivot to `ruzstd` 0.8 — pure-Rust, ships both `encoding::compress_to_vec` AND `decoding::StreamingDecoder`. Compression ratio / speed are below the C reference but the T39 acceptance criteria only require "Roundtrip compress → extract preserves files" — ratio is not on the surface. Documented inline in the root `Cargo.toml` workspace entry.
- **`zip` default-features = false**: the `zip` crate's default features pull in `zstd` (→ `cc-rs` + C libzstd) + `bzip2` + `lzma` + `ppmd` + `xz` + `aes-crypto` — every one of those either pulls a C dep OR is explicitly forbidden by the T39 task spec ("No 7z, RAR, BZip2, encryption-at-rest"). We enable ONLY `deflate` which uses `flate2` with the pure-Rust `miniz_oxide` backend. This drops support for zstd-compressed entries INSIDE zip files (still uncommon in the wild; deflate remains the dominant zip codec).
- **FFI safety**: every public entry point follows the 6 hard rules from `crates/buff-lang-ffi-guide/GUIDE.md`. See the compliance table in `src/lib.rs` module doc.
- **Panic-free**: no `unwrap` / `expect` / `panic!` / `todo!` / `unimplemented!` in non-test code. Every fallible op returns `Result<_, ArchiveError>`.
- **`catch_unwind` boundary**: all 4 public entry points (`compress_dir` / `extract` / `compress_bytes` / `decompress_bytes`) wrap their bodies in `catch_unwind` per FFI guide R6.
- **Buff §6 / §7 compliance**: NO `_async` suffix (synchronous surface); no `Type.create()` / `Type.build()` (constructors are `Type::new` only — and `Archive` is a namespace-only marker, never instantiated).

## RELATIONSHIP TO OTHER CRATES

| Crate | Relationship |
|---|---|
| `zip` | Upstream ZIP codec. `buff-archive` is a safe wrapper; never re-exports `zip::*` types directly. Deflate-only build via `default-features = false, features = ["deflate"]`. |
| `tar` | Upstream TAR codec. Already pinned at workspace level for T127 (CLI package publish) + T139 (buffup tarball unpack). |
| `flate2` | Upstream Gzip codec. Already pinned at workspace level for T139. Default `miniz_oxide` pure-Rust backend. |
| `ruzstd` | Upstream Zstd codec. Pure-Rust alternative to the C-based `zstd` crate. New workspace pin in T39. |
| `buff-lang-types` | (T39 wiring) `prelude_types.rs` registers `PreludeType::Archive` (namespace-only — `buff_type()` returns `Type::Void`) + `PreludeAssocFn::{CompressDir, Extract}`. `is_namespace_only()` includes `Archive`. |
| `buff-lang-codegen-rust` | (T39 wiring) `rust_codegen.rs::lower_prelude_type_assoc_fn` gains the `(Archive, CompressDir)` / `(Archive, Extract)` arms. `program_uses_namespace("Archive")` records `buff-archive` + `zip` + `tar` + `flate2` + `ruzstd` in `extern_crates`. |
| `buff-lang-ffi-guide` | Defines the 6 hard rules every public function in this crate follows. |

## NOTES

- **MSVC host blocker**: `cargo test -p buff-archive` fails on this Windows host with `LINK : fatal error LNK1104: cannot open file 'msvcrt.lib'` — pre-existing VS 18 Insiders + missing Windows SDK UCRT headers issue (same family that blocks `cargo check --workspace` here, documented in buff-image / AGENTS.md). The failure is during `zip`'s build script link step. CI runs on a 3-OS matrix (ubuntu/windows/macos) and does NOT have this issue.
- **Dir roundtrip = tar+codec for Gz/Zstd**: when `compress_dir` is called with `Gz` or `Zstd`, the directory is first packed into an uncompressed TAR and the codec is applied to the byte stream — producing `.tar.gz` / `.tar.zst`. This mirrors the cross-language convention (`tarfile` + `gzip` in Python, `tar -czf` / `tar --zstd -cf` on the CLI). `extract` auto-detects the codec from the file's extension and reverses the pipeline.
- **No streaming API for MVP**: `compress_bytes` / `decompress_bytes` allocate the full input + output in memory. A streaming variant (e.g. `compress_stream(reader, writer, format)`) is a v1.18+ enhancement — out of scope for the T39 MVP per the LOC/fn budget.
- **No encryption-at-rest**: explicitly forbidden by the T39 task spec ("combine with T34 if needed"). The `zip` crate's `aes-crypto` feature is disabled (would also pull C deps via `aes-soft`'s `aesni` backend on x86).
- **No 7z, RAR, BZip2**: explicitly forbidden by the T39 task spec. v1.22+ work.
