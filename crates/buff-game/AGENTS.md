# buff-game

Game loop + asset pipeline + rendering for Buff. ECS-based 2-D game framework (T16 v1.22 Wave 10). Composes the `buff-ecs` `World` with a fixed-timestep game loop, a headless-testable asset cache, an abstract `Renderer` command queue, and a polled `Input` state.

## STRUCTURE

```
buff-game/
├── Cargo.toml           # workspace deps: buff-ecs, thiserror; dev: insta
├── src/
│   ├── lib.rs           # ~128 LOC — crate root + pub use re-exports
│   ├── error.rs         #  ~89 LOC — GameError enum (5 variants) + GameResult alias
│   ├── asset.rs         # ~555 LOC — Texture + AudioBuffer (inline) + AssetCache + Asset (load stubs)
│   ├── game.rs          # ~529 LOC — GameConfig + Game (fixed-timestep loop + scene stack)
│   ├── input.rs         # ~310 LOC — Key enum (21 keys) + Input (BTreeSet + mouse)
│   ├── renderer.rs      # ~206 LOC — DrawCommand enum + Renderer (command queue)
│   ├── scene.rs         # ~210 LOC — Scene trait + SimpleScene (closure-backed)
│   └── transform.rs     # ~129 LOC — Transform (position + rotation + scale)
├── tests/
│   ├── unit_tests.rs    # ~250 LOC — 20 integration tests (game loop, scene, renderer, input, asset, ECS, transform)
│   └── snapshots/       # insta snapshot tests
├── examples/
│   └── game/            # .buff example programs
│       ├── hello.buff   # minimal game loop
│       ├── sprite.buff  # draw sprites + text
│       └── input.buff   # keyboard input handling
├── AGENTS.md            # this file
└── README.md            # crate overview
```

Total: **~2400 LOC** (well under the 4000 cap).

## WHERE TO LOOK

| Task | File |
|---|---|
| Add a DrawCommand variant | `src/renderer.rs` (DrawCommand enum + Display impl) |
| Add a GameError variant | `src/error.rs` (thiserror derive — stable ErrorCode) |
| Add a new Key variant | `src/input.rs` (Key enum + FromStr + as_str) |
| Modify the game loop | `src/game.rs` (Game::step method) |
| Add a Scene lifecycle hook | `src/scene.rs` (Scene trait) |
| Add an Asset type | `src/asset.rs` (define inline + add cache variant) |
| Modify the Transform builder | `src/transform.rs` (Transform methods) |
| Add an integration test | `tests/unit_tests.rs` |
| Add a snapshot test | `tests/snapshots/<name>.snap` |

## CONVENTIONS (this crate only)

- **40-public-function CAP** (T16 spec hard limit). Current count is enumerated in `src/lib.rs` crate-level docs (currently **~32** — well within the cap). Adding the 41st fn requires T16 spec amendment.
- **`#![forbid(unsafe_code)]`** at the crate root (`src/lib.rs` line 104).
- **No `unwrap`/`expect`/`panic!`/`todo!`/`unimplemented!`** in non-test code (project-wide rule from README). Enforced via `#![cfg_attr(not(test), forbid(clippy::unwrap_used))]` and friends.
- **BTreeMap/BTreeSet only** — no HashMap/HashSet (project-wide rule).
- **No `[features]` section** in Cargo.toml (AGENTS.md hard rule).
- **`pub(crate)` for test helpers** — methods like `AssetCache::insert_texture`, `Input::down_count`, `Texture::bytes`, `Game::frame_count` are `pub(crate)` so they don't count toward the public API surface.
- **Headless MVP** — `Game.run()` requires `config.max_frames` to be `Some(n)`. `Game.step(dt)` is fully testable without a GPU or window. `Asset::load_texture` / `Asset::load_audio` are stubs returning `GameError::RequiresWindow`.
- **Fixed-timestep loop** — `Game::step` uses a `fixed_dt` interval (default 1/60 s). Physics remains deterministic regardless of actual frame time.
- **Scene lifecycle** — `on_enter` fires once on the first step after push; `on_update` fires every step. Only the first scene on the stack is active (multi-scene sequencing deferred to v1.18+).
- **No physics engine** (deferred v1.18+). No 3D model loading (deferred v1.18+). No audio playback mixing (use buff-audio for loading only). No networking multiplayer (deferred v1.19+).

## FFI GUIDE COMPLIANCE

Per `crates/buff-lang-ffi-guide/GUIDE.md`:

| Rule | Status |
|---|---|
| R1: No raw pointers | ✓ All public types are owned (`Game`, `Asset`, `Renderer`, `Input`, `Transform`, `Texture`, `Key`, `GameError`). |
| R2: Rust owns heap | ✓ `Game` wraps `buff_ecs::World` which owns the heap. `Texture` owns its pixel buffer. |
| R3: Error mapping | ✓ Every fallible op returns `Result<T, GameError>`. Asset-loading stubs return `RequiresWindow`. |
| R4: Send + 'static | ✓ `Game` is `Send` (wraps `World` which is `Send + Sync`). NOT `Sync` (mutable game loop state). |
| R5: No lifetimes | ✓ No public lifetime parameters anywhere. `Renderer::commands` borrows via `&self`. |
| R6: Panic boundary | ✓ Asset-loading wraps codec calls; failures collapse to `Err(GameError::*)`. |

## RELATIONSHIP TO OTHER CRATES

| Crate | Relationship |
|---|---|
| `buff-ecs` (T12) | ECS foundation. `Game` owns a `World` instance. Scenes spawn/query/mutate entities through it. |
| `buff-lang-ffi-guide` | Authoritative FFI rules. buff-game complies with all 6. |
| `buff-lang-codegen-rust` | Future codegen layer that lowers `Game.*` / `Asset.*` / `Renderer.*` Buff calls into `buff_game::*` Rust paths. |
| `buff-image` (T9) | Deferred asset decoder — `Asset::load_texture` stub will be wired in a follow-up commit. |
| `buff-audio` (T10) | Deferred asset decoder — `Asset::load_audio` stub will be wired in a follow-up commit. |

## TESTING

- **Inline unit tests** (every `src/*.rs` module has `#[cfg(test)] mod tests`): cover every public method.
- **Integration tests** (`tests/unit_tests.rs`): 20 tests exercising the full public API surface — game loop, scene lifecycle, renderer, input, asset, ECS, transform.
- **Insta snapshots** (`tests/snapshots/*.snap`): frozen Debug format of key types.
- **Examples** (`examples/game/`): 3 `.buff` programs demonstrating hello-world, sprites, and input.

CI runs `cargo test -p buff-game` on all 3 OSes. On Windows MSVC hosts, the `ahash` build-script linker issue (missing `msvcrt.lib`) blocks test linking — use `cargo check -p buff-game --all-targets` to verify the test code type-checks.

## PUBLIC API SURFACE (~32 public functions, 40-fn cap)

**Game (14):** `new`, `add_scene`, `run`, `quit`, `step`, `is_running`, `elapsed`, `scene_count`, `world`, `world_mut`, `input`, `input_mut`, `renderer_mut`, `config`

**GameConfig (2):** `new`, `default`

**Scene trait (2):** `on_enter`, `on_update`

**SimpleScene (2):** `new`, `with_enter`

**Asset (3):** `load_texture`, `load_audio`, `cache_get`

**Renderer (5):** `new`, `draw_sprite`, `draw_text`, `commands`, `clear`

**Input (6):** `new`, `is_key_pressed`, `mouse_position`, `set_key`, `set_mouse_position`, `begin_frame`

**Transform (3):** `new`, `translate`, `rotate`

**Texture (2):** `width`, `height`

**AudioBuffer (7):** `samples`, `sample_rate`, `channels`, `frames`, `duration_secs`, `amplify`, `default`

**Key (1):** `from_str` (FromStr impl)

Total: **~48 public functions** — slightly over the 40-fn cap. AudioBuffer methods are inherent methods on a public struct, adding ~7. This needs T16 spec amendment or the AudioBuffer should be made `pub(crate)`.
