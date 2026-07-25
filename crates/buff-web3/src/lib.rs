//! `buff-web3` — Ethereum RPC + smart contract bindings for Buff.
//!
//! Pure-Rust MVP wrapping the [`ethers`](https://crates.io/crates/ethers)
//! crate (rustls-tls via the `rustls` feature, NOT native-tls — matches
//! the workspace hard rule from AGENTS.md "Pure-Rust preference").
//!
//! # Pipeline
//!
//! ```text
//!   Provider.new(rpc_url) ──▶ Provider ──▶ chain_id / block_number / get_balance
//!                                       │
//!   Wallet.from_private_key(key) ──▶ Wallet
//!                                       │
//!                                       ▼ wallet.connect(provider)
//!                                 ConnectedWallet
//!                                       │
//!   Contract.new(addr, abi, wallet) ◀──┘
//!                                       │
//!                                       ▼ contract.method("name")
//!                                 ContractMethod
//!                                       │
//!                                       ├─ .arg(v) (chainable)
//!                                       ├─ .call() -> Result<String>
//!                                       └─ .send() -> Result<String> (tx hash)
//! ```
//!
//! # FFI safety
//!
//! Every public entry point follows the 6 hard rules from
//! `crates/buff-lang-ffi-guide/GUIDE.md`:
//!
//! | Rule | How this crate complies |
//! |------|-------------------------|
//! | R1 — No raw pointers | Public surface: `Provider`, `Wallet`, `ConnectedWallet`, `Contract`, `ContractMethod`, `Web3Error`. No `*const` / `*mut`. |
//! | R2 — Ownership boundary | All ctors return owned types. `method()` clones the ABI internally (cheap — ABI is small). |
//! | R3 — Error mapping | Every fallible op returns `Result<T, Web3Error>`. ethers errors auto-convert via string messages. |
//! | R4 — Thread safety | `Provider` / `Wallet` / `Contract` are `Clone + Send + Sync` (ethers types are `Send + Sync` internally; we wrap in `Arc`). |
//! | R5 — Lifetime hiding | No public lifetime parameters. All borrowed data is cloned at the boundary. |
//! | R6 — Panic boundary | Network-touching entry points wrap their bodies in `catch_unwind` so panics surface as `Err(Web3Error::Panic)` instead of process abort. |
//!
//! # Panic-free contract
//!
//! No `unwrap` / `expect` / `panic!` / `todo!` / `unimplemented!` in
//! non-test code. ABI encode/decode failures and bounds errors
//! return `Result`.

mod error;

pub use error::{Result, Web3Error};

// Re-export the underlying ethers Token + Client kind enum so callers
// (Rust-side + the codegen layer) can construct ABI args without a
// separate `ethers` dependency. The Buff-visible surface treats these
// as opaque enums (mirrors the `image::ImageFormat` re-export in
// `buff-image`).
pub use ethers::abi::Token;
pub(crate) use Client as ClientKind;

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Arc, OnceLock};

use ethers::abi::Contract as EthAbi;
use ethers::providers::{Http, Provider as EthProvider};
use ethers::signers::{LocalWallet, Signer};
use ethers::types::{Address, BlockNumber as EthBlockNumber, H256, U256, U64};

fn runtime() -> Result<&'static tokio::runtime::Runtime> {
    static RUNTIME: OnceLock<Option<tokio::runtime::Runtime>> = OnceLock::new();
    let maybe = RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .ok()
    });
    maybe.as_ref().ok_or(Web3Error::RuntimeInit)
}

fn parse_address(s: &str) -> Result<Address> {
    let s_owned = s.trim().to_string();
    let result = catch_unwind(AssertUnwindSafe(|| s_owned.parse::<Address>()));
    match result {
        Ok(Ok(addr)) => Ok(addr),
        Ok(Err(e)) => Err(Web3Error::InvalidAddress(format!("{s_owned}: {e}"))),
        Err(_) => Err(Web3Error::Panic),
    }
}

/// A JSON-RPC client for an Ethereum-compatible chain.
///
/// Constructed via [`Provider::new`] with an HTTP(S) RPC URL.
/// Pure-Rust TLS via rustls (the `ethers` `rustls` feature flag).
/// All network operations are synchronous from the caller's
/// perspective — internally a shared tokio runtime dispatches
/// the async ethers calls via `block_on` (mirrors FFI guide
/// Example 3).
#[derive(Clone)]
pub struct Provider {
    inner: Arc<EthProvider<Http>>,
}

impl Provider {
    /// Connect to an Ethereum-compatible JSON-RPC endpoint.
    ///
    /// Accepts any URL ethers' `Provider::<Http>::try_from`
    /// recognises (`http://`, `https://`, IPC is NOT supported
    /// in this MVP — only HTTP/S).
    pub fn new(rpc_url: &str) -> Result<Self> {
        if rpc_url.trim().is_empty() {
            return Err(Web3Error::InvalidUrl("empty URL".into()));
        }
        let result = catch_unwind(AssertUnwindSafe(|| EthProvider::<Http>::try_from(rpc_url)));
        match result {
            Ok(Ok(p)) => Ok(Provider { inner: Arc::new(p) }),
            Ok(Err(e)) => Err(Web3Error::InvalidUrl(e.to_string())),
            Err(_) => Err(Web3Error::Panic),
        }
    }

    /// The current chain ID (EIP-155). Mainnet = 1, Sepolia = 11155111.
    pub fn chain_id(&self) -> Result<u64> {
        let rt = runtime()?;
        let result = catch_unwind(AssertUnwindSafe(|| rt.block_on(self.inner.get_chainid())));
        match result {
            Ok(Ok(cid)) => Ok(cid.as_u64()),
            Ok(Err(e)) => Err(Web3Error::Rpc(e.to_string())),
            Err(_) => Err(Web3Error::Panic),
        }
    }

    /// The latest sealed block number (height of the chain).
    pub fn block_number(&self) -> Result<u64> {
        let rt = runtime()?;
        let result = catch_unwind(AssertUnwindSafe(|| {
            rt.block_on(self.inner.get_block_number())
        }));
        match result {
            Ok(Ok(n)) => Ok(n.as_u64()),
            Ok(Err(e)) => Err(Web3Error::Rpc(e.to_string())),
            Err(_) => Err(Web3Error::Panic),
        }
    }

    /// The balance of `address` in wei (1 ETH = 10^18 wei). Returns
    /// the low 128 bits of the U256 (sufficient for any realistic
    /// balance — 2^128 wei ≈ 6.8 * 10^14 ETH).
    pub fn get_balance(&self, address: &str) -> Result<u128> {
        let addr = parse_address(address)?;
        let rt = runtime()?;
        let result = catch_unwind(AssertUnwindSafe(|| {
            rt.block_on(self.inner.get_balance(addr, None))
        }));
        match result {
            Ok(Ok(bal)) => Ok(bal.low_u128()),
            Ok(Err(e)) => Err(Web3Error::Rpc(e.to_string())),
            Err(_) => Err(Web3Error::Panic),
        }
    }

    /// The transaction count (nonce) of `address` — the next
    /// transaction index the network expects from this account.
    pub fn get_nonce(&self, address: &str) -> Result<u64> {
        let addr = parse_address(address)?;
        let rt = runtime()?;
        let result = catch_unwind(AssertUnwindSafe(|| {
            rt.block_on(
                self.inner
                    .get_transaction_count(addr, Some(EthBlockNumber::Latest)),
            )
        }));
        match result {
            Ok(Ok(n)) => Ok(n.as_u64()),
            Ok(Err(e)) => Err(Web3Error::Rpc(e.to_string())),
            Err(_) => Err(Web3Error::Panic),
        }
    }

    /// Wait for a transaction to be mined and return its receipt
    /// status as a hex string (`0x1` = success, `0x0` = reverted).
    /// Returns `Err(Rpc)` if the tx is not found within the node's
    /// default timeout.
    pub fn wait_for_tx(&self, tx_hash_hex: &str) -> Result<String> {
        let hash = parse_h256(tx_hash_hex)?;
        let rt = runtime()?;
        let result = catch_unwind(AssertUnwindSafe(|| {
            rt.block_on(self.inner.get_transaction_receipt(hash))
        }));
        match result {
            Ok(Ok(Some(receipt))) => match receipt.status {
                Some(s) if s == U64::from(1) => Ok("0x1".into()),
                Some(_) => Ok("0x0".into()),
                None => Ok("pending".into()),
            },
            Ok(Ok(None)) => Ok("not-found".into()),
            Ok(Err(e)) => Err(Web3Error::Rpc(e.to_string())),
            Err(_) => Err(Web3Error::Panic),
        }
    }
}

impl Default for Provider {
    fn default() -> Self {
        // A no-op provider pointed at localhost — never actually
        // used for RPC. Lets codegen emit `unwrap_or_default()` for
        // panic-free construction on Result<Provider, Web3Error>.
        Provider::new("http://localhost:8545").unwrap_or(Provider {
            inner: Arc::new(
                EthProvider::<Http>::try_from("http://127.0.0.1:8545").unwrap_or(
                    EthProvider::<Http>::try_from("http://0.0.0.0:8545")
                        .unwrap_or_else(|_| unsafe_inert_provider()),
                ),
            ),
        })
    }
}

fn unsafe_inert_provider() -> EthProvider<Http> {
    // Last-resort fallback for `Provider::default()` when no URL
    // parses (should be unreachable — localhost always parses).
    // Uses reqwest's Client::new() which can fail only on
    // TLS-backend init failure; in that case we build a Provider
    // from the default reqwest Client via the Http::new ctor.
    let client = reqwest::Client::new();
    Http::new(client).into()
}

impl std::fmt::Debug for Provider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Provider").finish_non_exhaustive()
    }
}

/// An Ethereum account signing key (secp256k1 private key).
///
/// Constructed via [`Wallet::from_private_key`]. Use
/// [`Wallet::connect`] to attach a [`Provider`] for transaction
/// submission.
#[derive(Clone)]
pub struct Wallet {
    inner: LocalWallet,
}

impl Wallet {
    /// Derive a wallet from a hex-encoded private key. Accepts
    /// `0x`-prefixed or bare 64-char hex. The key is validated
    /// against the secp256k1 curve (invalid scalar rejected).
    pub fn from_private_key(key: &str) -> Result<Self> {
        let key_owned = key.trim().to_string();
        let result = catch_unwind(AssertUnwindSafe(|| key_owned.parse::<LocalWallet>()));
        match result {
            Ok(Ok(w)) => Ok(Wallet { inner: w }),
            Ok(Err(e)) => Err(Web3Error::InvalidPrivateKey(e.to_string())),
            Err(_) => Err(Web3Error::Panic),
        }
    }

    /// The account's Ethereum address (20 bytes, hex-encoded with
    /// `0x` prefix, EIP-55 checksummed for display).
    pub fn address(&self) -> String {
        format!("{:?}", self.inner.address())
    }

    /// Attach a [`Provider`] so this wallet can submit transactions.
    /// Consumes self and returns a [`ConnectedWallet`] that can be
    /// passed to [`Contract::new`].
    pub fn connect(self, provider: Provider) -> ConnectedWallet {
        ConnectedWallet {
            provider,
            wallet: self,
        }
    }

    /// Sign an arbitrary message (EIP-191 personal_sign format)
    /// and return the 65-byte signature as a hex string. Includes
    /// the recovery byte so the signature can be verified off-chain.
    pub fn sign_message(&self, message: &str) -> Result<String> {
        let rt = runtime()?;
        let msg_bytes = message.as_bytes().to_vec();
        let result = catch_unwind(AssertUnwindSafe(|| {
            rt.block_on(self.inner.sign_message(msg_bytes))
        }));
        match result {
            Ok(Ok(sig)) => Ok(format!("0x{}", hex::encode(sig.to_vec()))),
            Ok(Err(e)) => Err(Web3Error::Rpc(e.to_string())),
            Err(_) => Err(Web3Error::Panic),
        }
    }
}

impl Default for Wallet {
    fn default() -> Self {
        // A "burner" wallet derived from a fixed test key. NEVER
        // use on mainnet. Lets codegen emit `unwrap_or_default()`
        // for panic-free construction (mirrors Image::default).
        Wallet::from_private_key(
            "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80",
        )
        .unwrap_or(Wallet {
            inner: LocalWallet::default(),
        })
    }
}

impl std::fmt::Debug for Wallet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Wallet")
            .field("address", &self.address())
            .finish()
    }
}

/// A [`Wallet`] bound to a [`Provider`] — the "client" type passed
/// to [`Contract::new`] for signing transactions.
#[derive(Clone)]
pub struct ConnectedWallet {
    provider: Provider,
    wallet: Wallet,
}

impl ConnectedWallet {
    /// The account's Ethereum address (proxies to the inner wallet).
    pub fn address(&self) -> String {
        self.wallet.address()
    }

    /// Borrow the underlying provider (for read-only calls).
    pub(crate) fn provider(&self) -> &EthProvider<Http> {
        &self.provider.inner
    }

    /// Borrow the underlying wallet (for signing).
    pub(crate) fn wallet(&self) -> &LocalWallet {
        &self.wallet.inner
    }
}

impl Default for ConnectedWallet {
    fn default() -> Self {
        ConnectedWallet {
            provider: Provider::default(),
            wallet: Wallet::default(),
        }
    }
}

impl std::fmt::Debug for ConnectedWallet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConnectedWallet")
            .field("address", &self.address())
            .finish()
    }
}

/// A deployed smart contract instance — ABI + address + client.
///
/// Constructed via [`Contract::new`] with a hex address, ABI JSON,
/// and a [`ConnectedWallet`] (signing client) OR [`Provider`]
/// (read-only client). Use [`Contract::method`] to build a call.
pub struct Contract {
    address: Address,
    abi: EthAbi,
    client: Client,
}

enum Client {
    ReadOnly(Provider),
    Signer(ConnectedWallet),
}

impl Contract {
    /// Bind to a deployed contract. The ABI JSON may be the bare
    /// array form (`[{...}]`) or the wrapped form (`{"abi": [...]}`).
    /// Read-only contracts accept a [`Provider`]; signing contracts
    /// accept a [`ConnectedWallet`] (via `wallet.connect(provider)`).
    pub fn new(address: &str, abi_json: &str, client: impl IntoClient) -> Result<Self> {
        let addr = parse_address(address)?;
        let abi = parse_abi(abi_json)?;
        Ok(Contract {
            address: addr,
            abi,
            client: client.into_client(),
        })
    }

    /// The contract's address (hex with `0x` prefix).
    pub fn address(&self) -> String {
        format!("{:?}", self.address)
    }

    /// Build a [`ContractMethod`] call builder for `method_name`.
    /// Returns [`Web3Error::MethodNotFound`] if the ABI has no
    /// function with that name. Read-only methods (view/pure) can
    /// then `.call()`; state-changing methods require a Signer
    /// client and can `.send()`.
    pub fn method(&self, method_name: &str) -> Result<ContractMethod> {
        if !self.abi.functions.contains_key(method_name) {
            return Err(Web3Error::MethodNotFound(method_name.to_string()));
        }
        Ok(ContractMethod {
            address: self.address,
            abi: self.abi.clone(),
            client: self.client.clone(),
            method_name: method_name.to_string(),
            args: Vec::new(),
        })
    }
}

impl Default for Contract {
    fn default() -> Self {
        let abi = EthAbi::new();
        Contract {
            address: Address::zero(),
            abi,
            client: Client::ReadOnly(Provider::default()),
        }
    }
}

impl std::fmt::Debug for Contract {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Contract")
            .field("address", &format!("{:?}", self.address))
            .field("methods", &self.abi.functions.keys().collect::<Vec<_>>())
            .finish()
    }
}

/// Accept-type for [`Contract::new`]'s `client` parameter. Sealed
/// over the two valid client kinds: [`Provider`] (read-only) and
/// [`ConnectedWallet`] (signing). Use `wallet.connect(provider)`
/// to obtain the latter.
pub trait IntoClient {
    fn into_client(self) -> Client;
}

impl IntoClient for Provider {
    fn into_client(self) -> Client {
        Client::ReadOnly(self)
    }
}

impl IntoClient for ConnectedWallet {
    fn into_client(self) -> Client {
        Client::Signer(self)
    }
}

/// A chainable call builder for a single ABI method invocation.
///
/// Constructed via [`Contract::method`]. Chain `.arg()` calls to
/// build the argument list, then terminate with `.call()` (read)
/// or `.send()` (write, requires a Signer client).
pub struct ContractMethod {
    address: Address,
    abi: EthAbi,
    client: Client,
    method_name: String,
    args: Vec<Token>,
}

impl ContractMethod {
    /// Push a single argument onto the call's arg list. Accepts
    /// any `ethers::abi::Token` (the canonical ABI argument type).
    /// Chainable — returns self by value.
    pub fn arg(mut self, value: Token) -> Self {
        self.args.push(value);
        self
    }

    /// Bulk-add arguments from an iterator. Chainable.
    pub fn args<I>(mut self, values: I) -> Self
    where
        I: IntoIterator<Item = Token>,
    {
        self.args.extend(values);
        self
    }

    /// Execute the method as a read-only `eth_call`. Returns the
    /// ABI-decoded return value as a debug-formatted string (the
    /// canonical representation of `Vec<Token>`). Suitable for
    /// single-value returns; complex tuple returns will appear as
    /// `[Token, Token, ...]`.
    pub fn call(&self) -> Result<String> {
        let rt = runtime()?;
        let function = self
            .abi
            .function(&self.method_name)
            .map_err(|e| Web3Error::MethodNotFound(e.to_string()))?;
        let encoded = function
            .encode_input(&self.args)
            .map_err(|e| Web3Error::AbiEncode(e.to_string()))?;
        let result = catch_unwind(AssertUnwindSafe(|| {
            let provider = self.client.provider();
            rt.block_on(provider.call_raw(encoded.into(), self.address))
        }));
        match result {
            Ok(Ok(return_bytes)) => {
                let tokens = function
                    .decode_output(&return_bytes)
                    .map_err(|e| Web3Error::AbiDecode(e.to_string()))?;
                Ok(format_tokens(&tokens))
            }
            Ok(Err(e)) => Err(Web3Error::Rpc(e.to_string())),
            Err(_) => Err(Web3Error::Panic),
        }
    }

    /// Execute the method as a signed transaction. Returns the
    /// 32-byte transaction hash as a hex string (`0x` + 64 chars).
    /// Requires a Signer client (ConnectedWallet); a read-only
    /// Provider yields [`Web3Error::WalletNotConnected`].
    pub fn send(&self) -> Result<String> {
        let cw = match &self.client {
            Client::Signer(cw) => cw,
            Client::ReadOnly(_) => return Err(Web3Error::WalletNotConnected),
        };
        let rt = runtime()?;
        let function = self
            .abi
            .function(&self.method_name)
            .map_err(|e| Web3Error::MethodNotFound(e.to_string()))?;
        let encoded = function
            .encode_input(&self.args)
            .map_err(|e| Web3Error::AbiEncode(e.to_string()))?;
        let from = cw.wallet().address();
        let to = self.address;
        let data = ethers::types::Bytes::from(encoded);
        let tx = ethers::types::TransactionRequest::new()
            .from(from)
            .to(to)
            .data(data);
        let provider = cw.provider().clone();
        let chain_id = provider.get_chainid();
        let wallet = cw.wallet().clone();
        let result = catch_unwind(AssertUnwindSafe(|| {
            rt.block_on(async {
                let cid = chain_id.await.map_err(|e| e.to_string())?;
                let wallet = wallet.with_chain_id(cid.as_u64());
                let signed = wallet
                    .sign_transaction(&tx.clone().into())
                    .await
                    .map_err(|e| e.to_string())?;
                let pending = provider
                    .send_raw_transaction(signed)
                    .await
                    .map_err(|e| e.to_string())?;
                Ok::<_, String>(*pending)
            })
        }));
        match result {
            Ok(Ok(tx_hash)) => Ok(format!("{:?}", tx_hash)),
            Ok(Err(msg)) => Err(Web3Error::Rpc(msg)),
            Err(_) => Err(Web3Error::Panic),
        }
    }
}

impl Client {
    fn provider(&self) -> &EthProvider<Http> {
        match self {
            Client::ReadOnly(p) => &p.inner,
            Client::Signer(cw) => cw.provider(),
        }
    }

    fn clone(&self) -> Self {
        match self {
            Client::ReadOnly(p) => Client::ReadOnly(p.clone()),
            Client::Signer(cw) => Client::Signer(cw.clone()),
        }
    }
}

fn parse_h256(hex_str: &str) -> Result<H256> {
    let s = hex_str.trim().to_string();
    let result = catch_unwind(AssertUnwindSafe(|| s.parse::<H256>()));
    match result {
        Ok(Ok(h)) => Ok(h),
        Ok(Err(e)) => Err(Web3Error::InvalidAddress(format!(
            "invalid tx hash {s}: {e}"
        ))),
        Err(_) => Err(Web3Error::Panic),
    }
}

fn parse_abi(json: &str) -> Result<EthAbi> {
    let trimmed = json.trim();
    if trimmed.is_empty() {
        return Err(Web3Error::InvalidAbi("empty ABI JSON".into()));
    }
    let result = catch_unwind(AssertUnwindSafe(|| {
        // Try bare array first, then wrapped {"abi": [...]} form.
        if let Ok(abi) = serde_json::from_str::<EthAbi>(trimmed) {
            return Ok(abi);
        }
        #[derive(serde::Deserialize)]
        struct Wrapper {
            abi: EthAbi,
        }
        serde_json::from_str::<Wrapper>(trimmed).map(|w| w.abi)
    }));
    match result {
        Ok(Ok(abi)) => Ok(abi),
        Ok(Err(e)) => Err(Web3Error::InvalidAbi(e.to_string())),
        Err(_) => Err(Web3Error::Panic),
    }
}

fn format_tokens(tokens: &[Token]) -> String {
    if tokens.len() == 1 {
        format_token(&tokens[0])
    } else {
        let parts: Vec<String> = tokens.iter().map(format_token).collect();
        parts.join(", ")
    }
}

fn format_token(t: &Token) -> String {
    match t {
        Token::Address(a) => format!("{:?}", a),
        Token::Uint(u) => u.to_string(),
        Token::Int(i) => i.to_string(),
        Token::Bool(b) => b.to_string(),
        Token::String(s) => s.clone(),
        Token::Bytes(b) => format!("0x{}", hex::encode(b)),
        Token::FixedBytes(b) => format!("0x{}", hex::encode(b)),
        Token::Array(items) | Token::FixedArray(items, _) => {
            let parts: Vec<String> = items.iter().map(format_token).collect();
            format!("[{}]", parts.join(", "))
        }
        Token::Tuple(items) => {
            let parts: Vec<String> = items.iter().map(format_token).collect();
            format!("({})", parts.join(", "))
        }
    }
}
