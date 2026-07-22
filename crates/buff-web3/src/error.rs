//! Error type for the `buff-web3` crate.
//!
//! All fallible operations surface as [`Web3Error`]. Each variant
//! carries enough context for a useful Buff-side diagnostic. The
//! error string is ready for the future `BuffError` migration
//! (error-prefixed strings per FFI guide §3 convention).
//!
//! # Panic-free contract
//!
//! No `unwrap` / `expect` / `panic!` / `todo!` / `unimplemented!`
//! in this module or any non-test code path. Per the T4 FFI guide
//! R6 (Panic Boundary), the public entry points use `catch_unwind`
//! so panics from the underlying `ethers` calls (e.g. malformed ABI
//! bytes during encode/decode) surface as `Err(Web3Error::Panic)`
//! instead of unwinding across the Buff boundary.

use thiserror::Error;

/// The single error type returned by every fallible `buff-web3`
/// operation. Implements `Display` via `thiserror` so the
/// `?`-propagation diagnostic reads cleanly.
#[derive(Debug, Error)]
pub enum Web3Error {
    /// RPC URL was empty or could not be parsed into a valid
    /// `reqwest::Url` by `ethers::Provider::<Http>::try_from`.
    #[error("invalid RPC URL: {0}")]
    InvalidUrl(String),

    /// Ethereum address was not valid hex / not 20 bytes.
    /// The expected Buff-surface form is `0x` followed by 40 hex
    /// chars (EIP-55 checksummed or lowercase).
    #[error("invalid Ethereum address (expected 0x + 40 hex chars): {0}")]
    InvalidAddress(String),

    /// Private key was not valid hex / not 32 bytes / failed
    /// secp256k1 curve check. The expected Buff-surface form is
    /// `0x` followed by 64 hex chars.
    #[error("invalid private key (expected 0x + 64 hex chars): {0}")]
    InvalidPrivateKey(String),

    /// ABI JSON failed to parse into `ethers::abi::Contract`.
    /// Covers: malformed JSON, missing `abi` envelope, unknown
    /// ABI type, etc. The original `serde_json` message is kept.
    #[error("invalid ABI JSON: {0}")]
    InvalidAbi(String),

    /// The JSON-RPC endpoint returned an error response.
    /// Covers: node not reachable, rate-limited, method not
    /// supported, block not found, etc. The original ethers
    /// `ProviderError` / `MiddlewareError` message is kept.
    #[error("RPC error: {0}")]
    Rpc(String),

    /// The requested method name was not found in the contract's
    /// ABI. Distinguishes typo from network failure.
    #[error("ABI method not found: {0}")]
    MethodNotFound(String),

    /// ABI argument encoding failed (wrong type / wrong count for
    /// the method's input tuple).
    #[error("ABI encode error: {0}")]
    AbiEncode(String),

    /// ABI return-value decoding failed (node returned bytes that
    /// do not match the method's declared output tuple).
    #[error("ABI decode error: {0}")]
    AbiDecode(String),

    /// `Contract.send(...)` was called on a Contract constructed
    /// from a read-only `Provider` (no `Wallet` connected).
    /// Read-only contracts can `call` but cannot sign transactions.
    #[error("wallet not connected: Contract.send requires a Wallet")]
    WalletNotConnected,

    /// `Contract.method(name)` was called on a Contract constructed
    /// without a connected wallet. Building a send-able method
    /// requires a signer.
    #[error("wallet not connected: Contract.method requires a Wallet")]
    MethodNeedsWallet,

    /// The tokio runtime could not be initialised (extremely rare —
    /// only happens on systems where thread spawning fails, e.g.
    /// resource exhaustion). Surfaced as a recoverable error rather
    /// than a process abort per FFI guide R6.
    #[error("tokio runtime initialization failed")]
    RuntimeInit,

    /// A wrapper-internal panic was caught by `catch_unwind` (per
    /// T4 FFI guide R6). The user sees a stable diagnostic instead
    /// of a process abort.
    #[error("internal error: web3 operation panicked")]
    Panic,
}

/// Convenience alias so callers can write `Result<T>` instead of
/// `Result<T, Web3Error>`. Mirrors the `buff-db` precedent.
pub type Result<T> = std::result::Result<T, Web3Error>;
