//! `buffup update` — self-update buffup.
//!
//! **Not yet implemented.** Self-update requires:
//!
//! 1. A buffup release channel (we'd publish buffup itself as a
//!    GitHub Release asset alongside the buff compiler binaries).
//! 2. A download-and-replace-self flow (in-place binary swap is
//!    non-trivial on Windows where the running .exe is locked).
//! 3. A version manifest (latest pointer) so the client knows what
//!    "newest" means without paginating the Releases API.
//!
//! Until that machinery ships (post-T139), the command prints
//! actionable guidance to stderr and returns
//! [`BuffupError::NotImplemented`]. The CLI exit code is non-zero,
//! which makes scripting failures obvious.

use crate::error::BuffupError;

/// Entry point for `buffup update`.
pub fn run() -> Result<(), BuffupError> {
    eprintln!("buffup: self-update is not yet implemented.");
    eprintln!(
        "       To update buffup, reinstall from source:\n         \
         cargo install --git https://github.com/buff-lang/buff buffup"
    );
    eprintln!(
        "       (Self-update is tracked as part of T139 follow-up; see\n        \
         `.sisyphus/plans/buff-post-v10-tooling.md` for the roadmap.)"
    );
    Err(BuffupError::NotImplemented("buffup self-update"))
}
