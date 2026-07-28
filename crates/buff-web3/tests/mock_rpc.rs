//! Mock JSON-RPC tests using httpmock (P6.4).
//!
//! These tests replace the `#[ignore]` network-dependent tests with
//! hermetic mock-server tests that simulate Ethereum JSON-RPC responses.
//! No anvil/hardhat node required.

use buff_web3::{Contract, Provider};
use httpmock::{Method, MockServer};
use serde_json::json;

/// Helper: stand up a mock that responds to a JSON-RPC method with a
/// given result value.
fn mock_rpc(server: &MockServer, _method: &str, result: serde_json::Value) {
    server.mock(|when, then| {
        when.method(Method::POST);
        then.status(200)
            .header("Content-Type", "application/json")
            .body(
                json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "result": result,
                })
                .to_string(),
            );
    });
}

#[test]
fn mock_chain_id() {
    let server = MockServer::start();
    mock_rpc(&server, "eth_chainId", json!("0x539")); // 1337 in hex

    let p = Provider::new(&server.base_url()).expect("provider");
    let cid = p.chain_id().expect("chain_id");
    assert_eq!(cid, 1337, "mock chain id should be 1337");
}

#[test]
fn mock_block_number() {
    let server = MockServer::start();
    mock_rpc(&server, "eth_blockNumber", json!("0x42")); // 66 in hex

    let p = Provider::new(&server.base_url()).expect("provider");
    let block = p.block_number().expect("block_number");
    assert_eq!(block, 66, "mock block number should be 66");
}

#[test]
fn mock_get_balance() {
    let server = MockServer::start();
    // 100 ETH in wei = 100 * 10^18 = 0x56BC75E2D63100000
    mock_rpc(&server, "eth_getBalance", json!("0x56bc75e2d63100000"));

    let p = Provider::new(&server.base_url()).expect("provider");
    let bal = p
        .get_balance("0x0000000000000000000000000000000000000000")
        .expect("balance");
    assert!(bal > 0, "mock balance should be positive, got {bal}");
}

#[test]
fn mock_contract_decimals_call() {
    let server = MockServer::start();

    // The eth_call response is ABI-encoded. For `decimals()` which
    // returns uint256, the encoded value 18 is a 32-byte big-endian.
    // ABI encoding of 18 = 0x000...0012 (18 in hex = 0x12)
    let encoded_18 = format!("0x{}12", "0".repeat(62)); // 64 hex chars total (32 bytes)

    mock_rpc(&server, "eth_call", json!(encoded_18));

    // Minimal ERC20 ABI with just the decimals() view function.
    let abi = r#"[{"constant":true,"inputs":[],"name":"decimals","outputs":[{"name":"","type":"uint8"}],"payable":false,"stateMutability":"view","type":"function"}]"#;

    let provider = Provider::new(&server.base_url()).expect("provider");
    let contract = Contract::new("0x0000000000000000000000000000000000000000", abi, provider)
        .expect("contract");

    let result = contract
        .method("decimals")
        .expect("method decimals")
        .call()
        .expect("call decimals");

    // The result is a debug-formatted string; it should contain "18"
    // somewhere in the output.
    assert!(
        result.contains("18") || result.contains("0x12"),
        "decimals result should contain 18 or 0x12: {result}"
    );
}

#[test]
fn mock_get_nonce() {
    let server = MockServer::start();
    mock_rpc(&server, "eth_getTransactionCount", json!("0x0")); // 0 transactions

    let p = Provider::new(&server.base_url()).expect("provider");
    let nonce = p
        .get_nonce("0x0000000000000000000000000000000000000000")
        .expect("nonce");
    assert_eq!(nonce, 0, "mock nonce should be 0");
}
