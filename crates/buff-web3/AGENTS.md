# buff-web3

Ethereum RPC + smart contract bindings for the Buff language. Pure-Rust MVP (CPU-only per Metis G7 lock — network I/O never runs on the GPU path). Wraps the [`ethers`](https://crates.io/crates/ethers) crate (rustls-tls via the `rustls` feature, NOT native-tls — matches the workspace hard rule from AGENTS.md "Pure-Rust preference"). Mirrors the surface shape of `web3.py` / `ethers.js` / `ethers-rs` / `Web3j`.

**Status: experimental** (T48 v1.18 frameworks wave 4).

## STRUCTURE

```
buff-web3/
├── Cargo.toml            # ethers (rustls) + reqwest + tokio + serde_json + hex + thiserror deps
├── src/
│   ├── lib.rs            # Provider + Wallet + ConnectedWallet + Contract + ContractMethod (~640 LOC)
│   └── error.rs          # Web3Error enum (~95 LOC)
└── tests/
    └── core.rs           # unit tests
```

## WHERE TO LOOK

| Task | File |
|---|---|
| Add a new web3 type (e.g. `Block`, `Transaction`, `Log`) | `src/lib.rs` (add struct + impl) + `crates/buff-lang-types/src/ty.rs` + `crates/buff-lang-types/src/prelude_types.rs` + `crates/buff-lang-codegen-rust/src/rust_codegen.rs` |
| Add a new error variant | `src/error.rs` |
| Add a new Provider / Wallet / Contract method | `src/lib.rs::<Type>::impl` block + `crates/buff-lang-types/src/prelude_types.rs` (PreludeInstanceFn + `instance_fn_return_type`) + `crates/buff-lang-codegen-rust/src/rust_codegen.rs::lower_prelude_type_instance_fn` |
| Add a new ctor (Type.new / Type.from_*) | `src/lib.rs::<Type>::impl` block + `crates/buff-lang-types/src/prelude_types.rs` (PreludeAssocFn + `assoc_fn_return_type`) + `crates/buff-lang-codegen-rust/src/rust_codegen.rs::lower_prelude_type_assoc_fn` |

## PUBLIC API

### `Provider` — Ethereum JSON-RPC client

| Method | Signature | Notes |
|---|---|---|
| `Provider::new` | `(rpc_url: &str) -> Result<Self, Web3Error>` | HTTP/S only (no IPC in MVP). `catch_unwind` boundary per FFI guide R6. |
| `provider.chain_id` | `(&self) -> Result<u64>` | EIP-155 chain ID. Mainnet = 1. |
| `provider.block_number` | `(&self) -> Result<u64>` | Latest sealed block height. |
| `provider.get_balance` | `(&self, address: &str) -> Result<u128>` | Low 128 bits of U256 wei balance. |
| `provider.get_nonce` | `(&self, address: &str) -> Result<u64>` | Next tx index for `address`. |
| `provider.wait_for_tx` | `(&self, tx_hash_hex: &str) -> Result<String>` | `"0x1"` (success) / `"0x0"` (reverted) / `"pending"` / `"not-found"`. |

### `Wallet` — secp256k1 signing key

| Method | Signature | Notes |
|---|---|---|
| `Wallet::from_private_key` | `(key: &str) -> Result<Self, Web3Error>` | Accepts `0x`-prefixed or bare 64-char hex. Curve-validated. |
| `wallet.address` | `(&self) -> String` | EIP-55 checksummed hex. |
| `wallet.connect` | `(self, provider: Provider) -> ConnectedWallet` | Consumes self. The signing-client ctor. |
| `wallet.sign_message` | `(&self, message: &str) -> Result<String>` | EIP-191 personal_sign. 65-byte sig as `0x`+hex. |

### `ConnectedWallet` — the signing client

| Method | Signature | Notes |
|---|---|---|
| `cw.address` | `(&self) -> String` | Proxies to the inner wallet. |
| `cw.provider` (pub(crate)) | `(&self) -> &EthProvider<Http>` | Internal borrow for read-only calls. |
| `cw.wallet` (pub(crate)) | `(&self) -> &LocalWallet` | Internal borrow for signing. |

### `Contract` — deployed smart contract instance

| Method | Signature | Notes |
|---|---|---|
| `Contract::new` | `(address: &str, abi_json: &str, client: impl IntoClient) -> Result<Self, Web3Error>` | Accepts bare array `[{...}]` or wrapped `{"abi":[...]}` form. `client` is `Provider` (read-only) OR `ConnectedWallet` (signing). |
| `contract.address` | `(&self) -> String` | Hex with `0x` prefix. |
| `contract.method` | `(&self, method_name: &str) -> Result<ContractMethod>` | Returns `MethodNotFound` if the ABI has no function with that name. |

### `ContractMethod` — chainable call builder

| Method | Signature | Notes |
|---|---|---|
| `m.arg` | `(self, value: Token) -> Self` | Push a single ABI arg. Chainable. |
| `m.args` | `(self, values: I) -> Self where I: IntoIterator<Item = Token>` | Bulk-add args. Chainable. |
| `m.call` | `(&self) -> Result<String>` | Read-only `eth_call`. ABI-decoded return as debug-formatted text. |
| `m.send` | `(&self) -> Result<String>` | Signed tx — 32-byte hash as `0x`+hex. Requires `ConnectedWallet`. |

## CONVENTIONS

- **Pure-Rust only**: wraps `ethers` (rustls feature) + `reqwest` (rustls-tls). NO `native-tls`, NO `openssl-sys`, NO cc-rs.
- **CPU-only per Metis G7 lock**: NO GPU dispatch (network I/O never runs on the GPU path).
- **FFI safety**: every public entry point follows the 6 hard rules from `crates/buff-lang-ffi-guide/GUIDE.md`. See the compliance table in `src/lib.rs` module doc.
- **Panic-free**: no `unwrap` / `expect` / `panic!` in non-test code. Network-touching entry points wrap `catch_unwind` per FFI guide R6.
- **Async hidden via tokio runtime**: the shared `tokio::runtime::Runtime` is stored in a `OnceLock` and accessed via `runtime()?.block_on(...)`. The Buff surface sees only synchronous calls — Buff's "no `await` keyword" rule is preserved.
- **Infallible on the Buff surface**: every fallible Rust ctor surfaces as infallible via codegen's `.unwrap_or_default()` collapse (Provider / Wallet / Contract all impl Default).

## RELATIONSHIP TO OTHER CRATES

- **Consumers**: `buff-lang-codegen-rust` lowers Buff `Provider.new(...)` / `Wallet.from_private_key(...)` / `contract.method(name).call()` etc. to `buff_web3::*` Rust paths.
- **Upstream**: `ethers` (the canonical Ethereum RPC + signer crate), `reqwest` (rustls-tls HTTP), `tokio` (multi-threaded runtime), `serde_json` (ABI JSON parser), `hex` (signature / tx-hash encoding).
- **Siblings**: mirrors the structure of T45 `buff-geo` (instance types with ctors + methods), T47 `buff-chat` (instance types with builder methods), T50 `buff-xml` (parse + instance accessors), T52 `buff-protobuf` (serialize / deserialize namespaces).
- **Auth overlap**: T34 `buff-auth` covers JWT / OAuth2 / Argon2 password hashing / RBAC. T48 does NOT cover wallet-based authentication (only transaction signing); future wallet-auth integration is a v1.20+ concern.

## CODEGEN INTEGRATION

The Buff surface (`Provider.new(url)` / `provider.chain_id()` / `Wallet.from_private_key(key)` / `wallet.connect(p)` / `Contract.new(addr, abi, wallet)` / `contract.method(name)` / `m.arg(name, value)` / `m.call()` / `m.send()`) is wired in:

- **Type variants**: `Type::Provider` / `Type::Wallet` / `Type::ConnectedWallet` / `Type::Contract` / `Type::ContractMethod` in `crates/buff-lang-types/src/ty.rs`
- **Prelude registry**: `PreludeType::{Provider, Wallet, ConnectedWallet, Contract, ContractMethod}` + `PreludeAssocFn::FromPrivateKey` + `PreludeInstanceFn::{ChainId, BlockNumber, GetBalance, GetNonce, WaitForTx, SignMessage, Method, Arg, Args, Call, Connect}` in `crates/buff-lang-types/src/prelude_types.rs`
- **Lowering**: `lower_prelude_type_assoc_fn` + `lower_prelude_type_instance_fn` in `crates/buff-lang-codegen-rust/src/rust_codegen.rs`
- **Extern crates**: `buff-web3` + `ethers` + `tokio` + `reqwest` + `serde_json` + `hex` registered via `program_uses_namespace("Provider")` / `("Wallet")` / `("ConnectedWallet")` / `("Contract")` / `("ContractMethod")` walkers
- **Tests**: `crates/buff-lang-codegen-rust/tests/web3_codegen.rs`

## NOTES

- **`ContractMethod.arg` name currently IGNORED at the wire layer**: the Buff surface accepts `m.arg(name: "_owner", value: "0x...")` (2-arg named form), but the codegen lowers only the value (`ethers::abi::Token::String((value).to_string())`) — the name is dropped because `ethers::abi::Token` doesn't carry names for non-tuple inputs. Future tuple-arg support (v1.20+) may consume the name for ABI-encoded tuple construction.
- **`Contract.send` requires `ConnectedWallet`**: a Contract constructed from a bare `Provider` (read-only) yields `Web3Error::WalletNotConnected` on `.send()`. The Buff surface surfaces this as the `String::default()` collapse via `.unwrap_or_default()` (panic-free). Use `wallet.connect(provider)` to obtain a `ConnectedWallet` before constructing the Contract.
- **`Provider::default` / `Wallet::default` exist ONLY for codegen's `.unwrap_or_default()` panic-free fallback**: `Provider::default()` points at `http://localhost:8545` (no-op); `Wallet::default()` is a "burner" wallet derived from a fixed test key (`0xac09...ff80` — the well-known Hardhat / Anvil test key). NEVER use the defaults on mainnet.
- **No IPC transport**: the MVP supports HTTP/S only (`Provider::<Http>::try_from`). IPC transport (`IpcProvider`) is a v1.20+ enhancement.
- **No WebSocket subscription streaming**: the MVP uses `block_on` for one-shot RPC calls. Streaming subscriptions (`eth_subscribe` / `eth_unsubscribe`) are a v1.20+ enhancement.
- **No ENS resolution**: the MVP accepts only hex addresses. ENS name resolution (`provider.resolve_name("vitalik.eth")`) is a v1.20+ enhancement.
- **No contract event listening**: the MVP supports only view/pure function calls + state-changing txs. Event log subscription is a v1.20+ enhancement.
- **No `ethers-contract` derive macro**: the MVP uses runtime ABI parsing (`Contract::new(addr, abi_json, client)`). The compile-time `#[eth_abi(...)]` derive is a v1.20+ enhancement.

## DEFERRED

- IPC transport (`IpcProvider`) (v1.20+)
- WebSocket subscription streaming (`eth_subscribe`) (v1.20+)
- ENS name resolution (v1.20+)
- Contract event log subscription (v1.20+)
- `ethers-contract` compile-time derive macro (v1.20+)
- Multi-sig wallet support (v1.22+)
- Hardware wallet (Ledger / Trezor) integration (v1.22+)
- Layer-2 specific bindings (Optimism / Arbitrum / Base) (v1.22+)
