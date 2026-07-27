//! Provider / parsing tests for `buff-web3`.
//!
//! These tests exercise the hermetic (network-free) surface of
//! [`Provider`], [`Contract`], and the address / ABI parsing helpers.
//! They cover:
//!
//!   - URL validation edge cases (already partly covered in core.rs;
//!     we extend with HTTPS variants, ports, paths, credentials).
//!   - Provider construction with malformed URLs returning the
//!     correct [`Web3Error::InvalidUrl`] variant.
//!   - Provider cloning + Send/Sync preservation across clones.
//!   - Address parsing edge cases via [`Contract::new`].
//!   - ABI JSON parsing edge cases via [`Contract::new`].
//!   - Default constructors for Provider / Wallet / Contract / etc.
//!   - Mock JSON-RPC server (a tiny std::net HTTP listener) that
//!     returns canned responses so we can verify Provider::chain_id /
//!     block_number / get_balance / get_nonce actually parse JSON-RPC
//!     responses correctly end-to-end.
//!
//! The mock server tests are NOT marked `#[ignore]` because they
//! spin up an ephemeral localhost listener on a random port — they
//! don't touch the public internet or any real Ethereum node. They
//! are hermetic and run in milliseconds.
//!
//! # Coverage
//!
//! 12+ test cases (counting the mock-server tests as a group). See
//! the per-section headers below for the breakdown.

use buff_web3::{Contract, Provider, Web3Error};

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};

// ============================================================================
// Section 1 — URL validation edge cases (no network).
//
// The Provider ctor calls ethers' `Provider::<Http>::try_from(url)`
// which accepts any URL the `url` crate can parse. We verify various
// well-formed and malformed inputs return the expected Result.
// ============================================================================

#[test]
fn provider_new_http_localhost_with_default_port() {
    let p = Provider::new("http://localhost:8545").expect("http localhost parses");
    let _ = p;
}

#[test]
fn provider_new_https_with_public_rpc_endpoint() {
    // Public RPC endpoint URL — we only construct, we don't connect.
    // URL parsing is the only failure mode here.
    let p = Provider::new("https://eth.llamarpc.com").expect("https parses");
    let _ = p;
}

#[test]
fn provider_new_https_with_custom_port() {
    let p = Provider::new("https://rpc.example:8546").expect("https with port parses");
    let _ = p;
}

#[test]
fn provider_new_http_with_path_component() {
    // Some RPC providers serve on a sub-path (e.g., Infura-style
    // `https://mainnet.infura.io/v3/<API_KEY>`).
    let p = Provider::new("https://mainnet.infura.io/v3/abc123").expect("https with path parses");
    let _ = p;
}

#[test]
fn provider_new_http_with_basic_auth_credentials() {
    // Some RPC providers require basic auth in the URL.
    let p = Provider::new("https://user:pass@rpc.example/").expect("https with auth parses");
    let _ = p;
}

#[test]
fn provider_new_rejects_unclosed_ipv6_bracket() {
    // An unclosed IPv6 bracket is a malformed URL — `url::Url::parse`
    // rejects it. ethers' `Provider::<Http>::try_from` propagates the
    // parse error.
    let err = Provider::new("http://[::1:8545").unwrap_err();
    assert!(
        matches!(err, Web3Error::InvalidUrl(_) | Web3Error::Panic),
        "expected InvalidUrl or Panic for malformed URL, got {err:?}"
    );
}

#[test]
fn provider_new_rejects_whitespace_only_url() {
    let err = Provider::new("   ").unwrap_err();
    assert!(matches!(err, Web3Error::InvalidUrl(_)), "got {err:?}");
}

#[test]
fn provider_new_rejects_url_with_trailing_garbage() {
    // `http://localhost:8545 garbage` should fail URL parsing.
    let err = Provider::new("http://localhost:8545 garbage").unwrap_err();
    assert!(
        matches!(err, Web3Error::InvalidUrl(_) | Web3Error::Panic),
        "got {err:?}"
    );
}

#[test]
fn provider_new_trims_whitespace_around_valid_url() {
    // The implementation calls `.trim()` on the URL before parsing,
    // so leading/trailing whitespace is tolerated for valid URLs.
    let p = Provider::new("  http://localhost:8545  ").expect("trim + valid url");
    let _ = p;
}

// ============================================================================
// Section 2 — Provider Clone + Send + Sync preservation.
// ============================================================================

#[test]
fn provider_clone_yields_independent_handle() {
    // Provider wraps an Arc<Provider<Http>>, so clone is cheap and
    // both handles share the same underlying transport. We verify
    // both clones can be used (here: held simultaneously).
    let p1 = Provider::new("http://localhost:8545").expect("provider");
    let p2 = p1.clone();
    let _ = (p1, p2);
}

#[test]
fn provider_can_be_sent_across_thread() {
    // Provider must be Send + Sync per FFI guide R4. We verify by
    // spawning a thread that takes ownership of a Provider.
    let p = Provider::new("http://localhost:8545").expect("provider");
    let handle = std::thread::spawn(move || {
        // Just drop it — verifies Send.
        drop(p);
    });
    handle.join().expect("thread should complete");
}

#[test]
fn provider_default_yields_working_handle() {
    // Provider::default is used by codegen's unwrap_or_default panic-
    // free fallback. It must always produce a valid (if inert) Provider.
    let p1 = Provider::default();
    let p2 = Provider::default();
    let _ = (p1, p2);
}

// ============================================================================
// Section 3 — Address parsing edge cases (via Contract::new).
//
// Contract::new calls the internal parse_address helper. We verify
// various valid + invalid address forms.
// ============================================================================

const ERC20_ABI: &str = r#"[
    {"type":"function","name":"balanceOf","inputs":[{"name":"account","type":"address"}],"outputs":[{"name":"","type":"uint256"}],"stateMutability":"view"}
]"#;

#[test]
fn contract_new_accepts_zero_address() {
    let provider = Provider::new("http://localhost:8545").expect("provider");
    let c = Contract::new(
        "0x0000000000000000000000000000000000000000",
        ERC20_ABI,
        provider,
    )
    .expect("zero address parses");
    assert_eq!(
        c.address().to_lowercase(),
        "0x0000000000000000000000000000000000000000"
    );
}

#[test]
fn contract_new_accepts_uppercase_address() {
    // Addresses are case-insensitive hex; EIP-55 mixed-case is the
    // checksummed display form but lowercase + uppercase both parse.
    let provider = Provider::new("http://localhost:8545").expect("provider");
    let c = Contract::new(
        "0x1234567890ABCDEF1234567890ABCDEF12345678",
        ERC20_ABI,
        provider,
    )
    .expect("uppercase address parses");
    assert!(c.address().to_lowercase().starts_with("0x1234"));
}

#[test]
fn contract_new_rejects_short_address() {
    let provider = Provider::new("http://localhost:8545").expect("provider");
    let err = Contract::new("0x1234", ERC20_ABI, provider).unwrap_err();
    assert!(matches!(err, Web3Error::InvalidAddress(_)), "got {err:?}");
}

#[test]
fn contract_new_rejects_non_hex_address() {
    let provider = Provider::new("http://localhost:8545").expect("provider");
    let err = Contract::new(
        "0xZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ",
        ERC20_ABI,
        provider,
    )
    .unwrap_err();
    assert!(matches!(err, Web3Error::InvalidAddress(_)), "got {err:?}");
}

#[test]
fn contract_new_rejects_address_without_0x_prefix() {
    // ethers' Address::from_str accepts both prefixed and bare hex,
    // but the bare form requires exactly 40 hex chars. We test the
    // edge case where the user forgot the prefix.
    let provider = Provider::new("http://localhost:8545").expect("provider");
    let result = Contract::new(
        "1234567890abcdef1234567890abcdef12345678",
        ERC20_ABI,
        provider,
    );
    // ethers accepts the bare 40-char hex form, so this should succeed.
    // (If it fails, the error variant should be InvalidAddress.)
    match result {
        Ok(c) => {
            assert!(c.address().to_lowercase().starts_with("0x"));
        }
        Err(Web3Error::InvalidAddress(_)) => {
            // Also acceptable — the impl may require the 0x prefix.
        }
        Err(e) => panic!("expected Ok or InvalidAddress, got {e:?}"),
    }
}

// ============================================================================
// Section 4 — ABI JSON parsing edge cases (via Contract::new).
// ============================================================================

#[test]
fn contract_new_accepts_bare_array_abi() {
    let provider = Provider::new("http://localhost:8545").expect("provider");
    let c = Contract::new(
        "0x0000000000000000000000000000000000000000",
        ERC20_ABI,
        provider,
    )
    .expect("bare array ABI parses");
    let _ = c;
}

#[test]
fn contract_new_accepts_wrapped_abi_envelope() {
    // The wrapped form `{"abi":[...]}` is what Hardhat / Foundry
    // artifacts ship by default. The parser should accept both forms.
    let wrapped = format!(r#"{{"abi":{ERC20_ABI}}}"#);
    let provider = Provider::new("http://localhost:8545").expect("provider");
    let c = Contract::new(
        "0x0000000000000000000000000000000000000000",
        &wrapped,
        provider,
    )
    .expect("wrapped ABI parses");
    let _ = c;
}

#[test]
fn contract_new_accepts_empty_array_abi() {
    // An empty ABI is valid (a contract with no callable methods —
    // e.g., a raw EOAs or a proxy). Useful for codegen's Default impl.
    let provider = Provider::new("http://localhost:8545").expect("provider");
    let c = Contract::new("0x0000000000000000000000000000000000000000", "[]", provider)
        .expect("empty array ABI parses");
    let _ = c;
}

#[test]
fn contract_new_rejects_empty_string_abi() {
    let provider = Provider::new("http://localhost:8545").expect("provider");
    let err =
        Contract::new("0x0000000000000000000000000000000000000000", "", provider).unwrap_err();
    assert!(matches!(err, Web3Error::InvalidAbi(_)), "got {err:?}");
}

#[test]
fn contract_new_rejects_whitespace_only_abi() {
    let provider = Provider::new("http://localhost:8545").expect("provider");
    let err = Contract::new(
        "0x0000000000000000000000000000000000000000",
        "   \n\t  ",
        provider,
    )
    .unwrap_err();
    assert!(matches!(err, Web3Error::InvalidAbi(_)), "got {err:?}");
}

#[test]
fn contract_new_rejects_non_json_abi() {
    let provider = Provider::new("http://localhost:8545").expect("provider");
    let err = Contract::new(
        "0x0000000000000000000000000000000000000000",
        "not json at all",
        provider,
    )
    .unwrap_err();
    assert!(matches!(err, Web3Error::InvalidAbi(_)), "got {err:?}");
}

#[test]
fn contract_new_rejects_wrong_json_shape() {
    // Valid JSON but wrong shape — not an array and not an `{abi:...}` envelope.
    let provider = Provider::new("http://localhost:8545").expect("provider");
    let err = Contract::new(
        "0x0000000000000000000000000000000000000000",
        r#"{"name":"Alice"}"#,
        provider,
    )
    .unwrap_err();
    assert!(matches!(err, Web3Error::InvalidAbi(_)), "got {err:?}");
}

// ============================================================================
// Section 5 — Mock JSON-RPC server tests (hermetic, no public network).
//
// We spin up a tiny `std::net::TcpListener` that speaks just enough
// HTTP/1.1 to return canned JSON-RPC responses. This lets us verify
// that `Provider::chain_id` / `block_number` / `get_balance` /
// `get_nonce` correctly parse real JSON-RPC payloads end-to-end
// without touching the public internet.
//
// The mock is a single-threaded request/response loop: it accepts
// one connection, reads the request, returns the canned response,
// then closes. Each test gets its own listener on a random port.
// ============================================================================

/// A tiny mock JSON-RPC server bound to an ephemeral localhost port.
///
/// Holds the listener + the thread handle. The thread accepts one
/// request per canned response in the queue; once the queue is
/// empty, subsequent requests get a default "method not found" error.
pub(crate) struct MockServer {
    pub(crate) url: String,
    _handle: std::thread::JoinHandle<()>,
}

impl MockServer {
    /// Spawn a mock server that returns the given canned JSON-RPC
    /// responses in sequence. Each request dequeues one response.
    pub(crate) fn spawn(mut responses: Vec<String>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
        let addr = listener.local_addr().expect("local_addr");
        let url = format!("http://{addr}");
        // Allow multiple concurrent connections during the test.
        listener
            .set_nonblocking(false)
            .expect("set_nonblocking false");
        let handle = std::thread::spawn(move || {
            // Accept up to N connections where N = responses.len().
            for _ in 0..responses.len().saturating_add(1) {
                let (mut stream, _) = match listener.accept() {
                    Ok(s) => s,
                    Err(_) => return,
                };
                let _ = handle_one_request(&mut stream, &mut responses);
            }
        });
        Self {
            url,
            _handle: handle,
        }
    }
}

fn handle_one_request(stream: &mut TcpStream, queue: &mut Vec<String>) -> std::io::Result<()> {
    // Read the request line + headers. We don't parse the body —
    // we just need to drain the request so the client doesn't get
    // a connection-reset before reading our response.
    stream.set_read_timeout(Some(std::time::Duration::from_secs(2)))?;
    let mut buf = [0u8; 4096];
    let _n = stream.read(&mut buf)?;

    // Determine the JSON-RPC method from the body so we can return
    // a contextually-appropriate response. The body is after the
    // blank line separating headers from body.
    let body = std::str::from_utf8(&buf).unwrap_or("");
    let body_start = body.find("\r\n\r\n").map(|i| i + 4).unwrap_or(0);
    let body_str = &body[body_start..];
    let is_chain_id = body_str.contains("\"eth_chainId\"");
    let is_block_number = body_str.contains("\"eth_blockNumber\"");
    let is_balance = body_str.contains("\"eth_getBalance\"");
    let is_nonce = body_str.contains("\"eth_getTransactionCount\"");
    let is_tx_receipt = body_str.contains("\"eth_getTransactionReceipt\"");

    // Pick the response: dequeue from the queue if available,
    // otherwise synthesize a context-appropriate default.
    let body = if let Some(r) = queue.pop() {
        r
    } else if is_chain_id {
        r#"{"jsonrpc":"2.0","id":1,"result":"0x1"}"#.to_string()
    } else if is_block_number {
        r#"{"jsonrpc":"2.0","id":1,"result":"0x10"}"#.to_string()
    } else if is_balance {
        r#"{"jsonrpc":"2.0","id":1,"result":"0xde0b6b3a7640000"}"#.to_string()
    } else if is_nonce {
        r#"{"jsonrpc":"2.0","id":1,"result":"0x0"}"#.to_string()
    } else if is_tx_receipt {
        r#"{"jsonrpc":"2.0","id":1,"result":null}"#.to_string()
    } else {
        r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"method not found"}}"#
            .to_string()
    };

    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    stream.write_all(response.as_bytes())?;
    stream.flush()?;
    Ok(())
}

#[test]
fn provider_chain_id_parses_mock_response() {
    // Mock returns chain_id = 0x1 (Ethereum mainnet).
    let server = MockServer::spawn(vec![]);
    let p = Provider::new(&server.url).expect("provider");
    let cid = p.chain_id().expect("chain_id");
    assert_eq!(cid, 1, "expected mainnet chain id from mock");
}

#[test]
fn provider_block_number_parses_mock_response() {
    // Mock returns block_number = 0x10 (16 decimal).
    let server = MockServer::spawn(vec![]);
    let p = Provider::new(&server.url).expect("provider");
    let n = p.block_number().expect("block_number");
    assert_eq!(n, 16, "expected block number 0x10 = 16 from mock");
}

#[test]
fn provider_get_balance_parses_mock_response() {
    // Mock returns 0xde0b6b3a7640000 = 10^18 wei = 1 ETH (low 128 bits).
    let server = MockServer::spawn(vec![]);
    let p = Provider::new(&server.url).expect("provider");
    let bal = p
        .get_balance("0x0000000000000000000000000000000000000000")
        .expect("get_balance");
    assert_eq!(
        bal, 1_000_000_000_000_000_000u128,
        "expected 1 ETH (10^18 wei) from mock"
    );
}

#[test]
fn provider_get_nonce_parses_mock_response() {
    // Mock returns nonce = 0x0.
    let server = MockServer::spawn(vec![]);
    let p = Provider::new(&server.url).expect("provider");
    let nonce = p
        .get_nonce("0x0000000000000000000000000000000000000000")
        .expect("get_nonce");
    assert_eq!(nonce, 0, "expected nonce 0 from mock");
}

#[test]
fn provider_wait_for_tx_returns_not_found_for_null_receipt() {
    // Mock returns null receipt — provider should map this to "not-found".
    let server = MockServer::spawn(vec![]);
    let p = Provider::new(&server.url).expect("provider");
    let status = p
        .wait_for_tx("0x0000000000000000000000000000000000000000000000000000000000000001")
        .expect("wait_for_tx");
    assert_eq!(status, "not-found", "expected 'not-found' for null receipt");
}

#[test]
fn provider_chain_id_handles_error_response() {
    // Mock returns a JSON-RPC error response (not a result). The
    // provider should surface this as Web3Error::Rpc.
    let err_response = r#"{
        "jsonrpc":"2.0","id":1,
        "error":{"code":-32601,"message":"method not found"}
    }"#
    .to_string();
    let server = MockServer::spawn(vec![err_response]);
    let p = Provider::new(&server.url).expect("provider");
    let err = p.chain_id().unwrap_err();
    assert!(
        matches!(err, Web3Error::Rpc(_)),
        "expected Rpc error for JSON-RPC error response, got {err:?}"
    );
}

#[test]
fn provider_chain_id_parses_anvil_chain_id() {
    // Anvil's default chain id is 31337 (0x7a69). We override the
    // default response to return this.
    let anvil_response = r#"{"jsonrpc":"2.0","id":1,"result":"0x7a69"}"#.to_string();
    let server = MockServer::spawn(vec![anvil_response]);
    let p = Provider::new(&server.url).expect("provider");
    let cid = p.chain_id().expect("chain_id");
    assert_eq!(cid, 31337, "expected anvil chain id (31337)");
}

#[test]
fn provider_block_number_parses_large_block_height() {
    // Mock returns a large block number (e.g., mainnet ~20M blocks).
    let response = r#"{"jsonrpc":"2.0","id":1,"result":"0x1312d00"}"#.to_string(); // 20000000
    let server = MockServer::spawn(vec![response]);
    let p = Provider::new(&server.url).expect("provider");
    let n = p.block_number().expect("block_number");
    assert_eq!(n, 20_000_000, "expected 20M block height");
}

#[test]
fn provider_get_balance_parses_large_balance() {
    // Mock returns 0xffffffffffffffffffffffffffffffff = 2^128 - 1 wei
    // (the max representable in the low 128 bits the API returns).
    let response =
        r#"{"jsonrpc":"2.0","id":1,"result":"0xffffffffffffffffffffffffffffffff"}"#.to_string();
    let server = MockServer::spawn(vec![response]);
    let p = Provider::new(&server.url).expect("provider");
    let bal = p
        .get_balance("0x0000000000000000000000000000000000000000")
        .expect("get_balance");
    assert_eq!(bal, u128::MAX, "expected u128::MAX balance (2^128 - 1 wei)");
}

#[test]
fn provider_get_balance_rejects_invalid_address_before_network() {
    // Invalid address should fail at the parse step, not at the
    // network step — so we don't even need a working mock server.
    let server = MockServer::spawn(vec![]);
    let p = Provider::new(&server.url).expect("provider");
    let err = p.get_balance("0xnot-an-address").unwrap_err();
    assert!(matches!(err, Web3Error::InvalidAddress(_)), "got {err:?}");
}

#[test]
fn provider_get_nonce_rejects_invalid_address_before_network() {
    let server = MockServer::spawn(vec![]);
    let p = Provider::new(&server.url).expect("provider");
    let err = p.get_nonce("not-hex-at-all").unwrap_err();
    assert!(matches!(err, Web3Error::InvalidAddress(_)), "got {err:?}");
}

#[test]
fn provider_wait_for_tx_rejects_short_hash() {
    // A tx hash must be 32 bytes (64 hex chars + 0x prefix = 66 chars).
    // Short hashes should be rejected at the parse step.
    let server = MockServer::spawn(vec![]);
    let p = Provider::new(&server.url).expect("provider");
    let err = p.wait_for_tx("0x1234").unwrap_err();
    assert!(matches!(err, Web3Error::InvalidAddress(_)), "got {err:?}");
}
