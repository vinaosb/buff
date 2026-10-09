//! Build script for the `buff` binary — PE main-thread stack parity (BUG-15).
//!
//! clap-derive inlines every subcommand's builder chain into the single
//! generated function
//! `<Command as clap::Subcommand>::augment_subcommands`. In debug builds
//! LLVM does not overlap the lifetimes of the resulting ~150 `Arg`
//! intermediates, so that one function reserves a ~983 KB stack frame —
//! which fits the 8 MB main-thread stacks Linux/macOS grant every process
//! by default, but overflows the 1 MB reserve the MSVC linker stamps into
//! PE binaries when rustc leaves it unspecified (verified: even a plain
//! `fn main() {}` rustc binary reserves exactly 1 MB). Every `buff`
//! invocation on Windows then died with STATUS_STACK_OVERFLOW inside
//! `Cli::parse()` — before any subcommand ran.
//!
//! The root of the divergence is the undeclared stack budget, so the fix
//! declares the same 8 MB budget Unix grants implicitly, via the PE
//! header (`/STACK:8388608`). Reserving address space is free — pages
//! are committed only on touch.

fn main() {
    // Stack parity only concerns the PE binaries produced for MSVC
    // targets; other hosts already default to 8 MB main-thread stacks.
    let target_env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if target_env == "msvc" {
        println!("cargo:rustc-link-arg-bins=/STACK:8388608");
    }
}
