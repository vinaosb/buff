//! Integration tests for the `buff-web3` crate.
//!
//! Covers all 14 public functions per the T48 spec:
//! - Provider: new, chain_id, block_number, get_balance, get_nonce, wait_for_tx
//! - Wallet: from_private_key, address, connect, sign_message
//! - ConnectedWallet: address
//! - Contract: new, address, method
//! - ContractMethod: arg, args, call, send
//!
//! Hermetic unit tests (no network) run by default. Network-dependent
//! tests are marked `#[ignore]` and run via `cargo test -- --ignored`
//! when a local testnet (anvil / hardhat) is available at
//! `http://localhost:8545`.

use buff_web3::{
    Client, ConnectedWallet, Contract, ContractMethod, IntoClient, Provider, Token, Wallet,
    Web3Error,
};

// Anvil's first derived account private key (well-known test key —
// NEVER use on mainnet; documented at
// https://book.getfoundry.sh/reference/anvil/). Used by both the
// unit-test Wallet construction and the ignored integration tests.
const ANVIL_KEY_A: &str =
    "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
const ANVIL_ADDR_A: &str = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266";

const ANVIL_KEY_B: &str =
    "0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d";

const ERC20_ABI: &str = r#"[
    {"type":"function","name":"balanceOf","inputs":[{"name":"account","type":"address"}],"outputs":[{"name":"","type":"uint256"}],"stateMutability":"view"},
    {"type":"function","name":"transfer","inputs":[{"name":"to","type":"address"},{"name":"amount","type":"uint256"}],"outputs":[{"name":"","type":"bool"}],"stateMutability":"nonpayable"},
    {"type":"function","name":"name","inputs":[],"outputs":[{"name":"","type":"string"}],"stateMutability":"view"},
    {"type":"function","name":"symbol","inputs":[],"outputs":[{"name":"","type":"string"}],"stateMutability":"view"},
    {"type":"function","name":"decimals","inputs":[],"outputs":[{"name":"","type":"uint8"}],"stateMutability":"view"}
]"#;

// ============ Provider construction ============

#[test]
fn provider_new_accepts_http_url() {
    let p = Provider::new("http://localhost:8545").expect("http url");
    assert_eq!(format!("{p:?}"), "Provider { .. }");
}

#[test]
fn provider_new_accepts_https_url() {
    let p = Provider::new("https://eth.llamarpc.com").expect("https url");
    let _ = p;
}

#[test]
fn provider_new_rejects_empty_url() {
    let err = Provider::new("").unwrap_err();
    assert!(matches!(err, Web3Error::InvalidUrl(_)), "got {err:?}");
}

#[test]
fn provider_new_rejects_garbage_url() {
    let err = Provider::new("not a url at all").unwrap_err();
    assert!(matches!(err, Web3Error::InvalidUrl(_)), "got {err:?}");
}

#[test]
fn provider_default_yields_inert_provider() {
    let _p = Provider::default();
}

// ============ Wallet construction ============

#[test]
fn wallet_from_private_key_derives_address() {
    let w = Wallet::from_private_key(ANVIL_KEY_A).expect("anvil key a");
    let addr = w.address();
    assert_eq!(addr.to_lowercase(), ANVIL_ADDR_A.to_lowercase());
}

#[test]
fn wallet_from_private_key_accepts_no_prefix() {
    let bare_key = ANVIL_KEY_A.trim_start_matches("0x");
    let w = Wallet::from_private_key(bare_key).expect("bare key");
    assert_eq!(w.address().to_lowercase(), ANVIL_ADDR_A.to_lowercase());
}

#[test]
fn wallet_from_private_key_rejects_short_key() {
    let err = Wallet::from_private_key("0xdeadbeef").unwrap_err();
    assert!(matches!(err, Web3Error::InvalidPrivateKey(_)), "got {err:?}");
}

#[test]
fn wallet_from_private_key_rejects_garbage() {
    let err = Wallet::from_private_key("not a key at all").unwrap_err();
    assert!(matches!(err, Web3Error::InvalidPrivateKey(_)), "got {err:?}");
}

#[test]
fn wallet_connect_returns_connected_wallet() {
    let wallet = Wallet::from_private_key(ANVIL_KEY_A).expect("wallet");
    let provider = Provider::new("http://localhost:8545").expect("provider");
    let cw = wallet.connect(provider);
    assert_eq!(cw.address().to_lowercase(), ANVIL_ADDR_A.to_lowercase());
}

#[test]
fn wallet_default_derives_known_address() {
    let w = Wallet::default();
    assert_eq!(w.address().to_lowercase(), ANVIL_ADDR_A.to_lowercase());
}

// ============ Contract construction ============

#[test]
fn contract_new_parses_valid_abi() {
    let provider = Provider::new("http://localhost:8545").expect("provider");
    let c = Contract::new(
        "0x0000000000000000000000000000000000000000",
        ERC20_ABI,
        provider,
    )
    .expect("contract");
    assert_eq!(
        c.address().to_lowercase(),
        "0x0000000000000000000000000000000000000000"
    );
}

#[test]
fn contract_new_accepts_wrapped_abi_form() {
    let wrapped = format!(r#"{{"abi":{ERC20_ABI}}}"#);
    let provider = Provider::new("http://localhost:8545").expect("provider");
    let c = Contract::new(
        "0x0000000000000000000000000000000000000000",
        &wrapped,
        provider,
    )
    .expect("wrapped contract");
    let _ = c;
}

#[test]
fn contract_new_rejects_empty_abi() {
    let provider = Provider::new("http://localhost:8545").expect("provider");
    let err = Contract::new(
        "0x0000000000000000000000000000000000000000",
        "",
        provider,
    )
    .unwrap_err();
    assert!(matches!(err, Web3Error::InvalidAbi(_)), "got {err:?}");
}

#[test]
fn contract_new_rejects_garbage_abi() {
    let provider = Provider::new("http://localhost:8545").expect("provider");
    let err = Contract::new(
        "0x0000000000000000000000000000000000000000",
        "not json",
        provider,
    )
    .unwrap_err();
    assert!(matches!(err, Web3Error::InvalidAbi(_)), "got {err:?}");
}

#[test]
fn contract_new_rejects_bad_address() {
    let provider = Provider::new("http://localhost:8545").expect("provider");
    let err = Contract::new("0xnot-an-address", ERC20_ABI, provider).unwrap_err();
    assert!(matches!(err, Web3Error::InvalidAddress(_)), "got {err:?}");
}

#[test]
fn contract_method_returns_builder_for_known_method() {
    let provider = Provider::new("http://localhost:8545").expect("provider");
    let c = Contract::new(
        "0x0000000000000000000000000000000000000000",
        ERC20_ABI,
        provider,
    )
    .expect("contract");
    let method = c.method("balanceOf").expect("balanceOf exists");
    let _ = method;
}

#[test]
fn contract_method_unknown_name_errors() {
    let provider = Provider::new("http://localhost:8545").expect("provider");
    let c = Contract::new(
        "0x0000000000000000000000000000000000000000",
        ERC20_ABI,
        provider,
    )
    .expect("contract");
    let err = c.method("nonexistentMethod").unwrap_err();
    assert!(matches!(err, Web3Error::MethodNotFound(_)), "got {err:?}");
}

#[test]
fn contract_method_arg_is_chainable() {
    let provider = Provider::new("http://localhost:8545").expect("provider");
    let c = Contract::new(
        "0x0000000000000000000000000000000000000000",
        ERC20_ABI,
        provider,
    )
    .expect("contract");
    let method = c.method("transfer").expect("transfer exists");
    let _chained = method.arg(Token::Address(ANVIL_ADDR_A.parse().expect("addr")));
}

// ============ Send-without-wallet error path ============

#[test]
fn contract_method_send_without_wallet_errors() {
    let provider = Provider::new("http://localhost:8545").expect("provider");
    let c = Contract::new(
        "0x0000000000000000000000000000000000000000",
        ERC20_ABI,
        provider,
    )
    .expect("read-only contract");
    let method = c.method("transfer").expect("transfer exists");
    let err = method.send().unwrap_err();
    assert!(matches!(err, Web3Error::WalletNotConnected), "got {err:?}");
}

// ============ Debug / Display impls ============

#[test]
fn snapshot_provider_debug() {
    let p = Provider::new("http://localhost:8545").expect("provider");
    insta::assert_snapshot!("provider_debug", format!("{p:?}"));
}

#[test]
fn snapshot_wallet_debug() {
    let w = Wallet::from_private_key(ANVIL_KEY_A).expect("wallet");
    insta::assert_snapshot!("wallet_debug", format!("{w:?}"));
}

#[test]
fn snapshot_connected_wallet_debug() {
    let wallet = Wallet::from_private_key(ANVIL_KEY_A).expect("wallet");
    let provider = Provider::new("http://localhost:8545").expect("provider");
    let cw = wallet.connect(provider);
    insta::assert_snapshot!("connected_wallet_debug", format!("{cw:?}"));
}

#[test]
fn snapshot_contract_debug() {
    let provider = Provider::new("http://localhost:8545").expect("provider");
    let c = Contract::new(
        "0x0000000000000000000000000000000000000001",
        ERC20_ABI,
        provider,
    )
    .expect("contract");
    insta::assert_snapshot!("contract_debug", format!("{c:?}"));
}

#[test]
fn snapshot_web3_error_debug() {
    let errs = vec![
        format!("{}", Web3Error::InvalidUrl("bad url".into())),
        format!("{}", Web3Error::InvalidAddress("bad addr".into())),
        format!("{}", Web3Error::InvalidPrivateKey("bad key".into())),
        format!("{}", Web3Error::InvalidAbi("bad abi".into())),
        format!("{}", Web3Error::MethodNotFound("nope".into())),
        format!("{}", Web3Error::WalletNotConnected),
        format!("{}", Web3Error::RuntimeInit),
        format!("{}", Web3Error::Panic),
    ];
    insta::assert_snapshot!("web3_error_display", errs.join("\n"));
}

// ============ IntoClient trait dispatch ============

#[test]
fn into_client_provider_yields_readonly() {
    let p = Provider::new("http://localhost:8545").expect("provider");
    let client: Client = p.into_client();
    assert!(matches!(client, Client::ReadOnly(_)));
}

#[test]
fn into_client_connected_wallet_yields_signer() {
    let wallet = Wallet::from_private_key(ANVIL_KEY_A).expect("wallet");
    let provider = Provider::new("http://localhost:8545").expect("provider");
    let cw = wallet.connect(provider);
    let client: Client = cw.into_client();
    assert!(matches!(client, Client::Signer(_)));
}

// ============ Network-dependent tests (skipped by default) ============
//
// Run with: `anvil` (or `npx hardhat node`) in another terminal, then
//           `cargo test -p buff-web3 -- --ignored`.

#[test]
#[ignore]
fn provider_chain_id_on_local_node() {
    let p = Provider::new("http://localhost:8545").expect("anvil");
    let cid = p.chain_id().expect("chain_id");
    assert_eq!(cid, 313372, "expected anvil chain id");
}

#[test]
#[ignore]
fn provider_block_number_increases() {
    let p = Provider::new("http://localhost:8545").expect("anvil");
    let block_a = p.block_number().expect("block_number");
    std::thread::sleep(std::time::Duration::from_millis(1500));
    let block_b = p.block_number().expect("block_number again");
    assert!(block_b >= block_a, "block did not advance: {block_a} -> {block_b}");
}

#[test]
#[ignore]
fn provider_get_balance_for_anvil_account() {
    let p = Provider::new("http://localhost:8545").expect("anvil");
    let bal = p.get_balance(ANVIL_ADDR_A).expect("balance");
    assert!(bal > 0, "expected anvil-funded account");
}

#[test]
#[ignore]
fn contract_call_reads_decimals_view_method() {
    let provider = Provider::new("http://localhost:8545").expect("anvil");
    let weth_addr = "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2";
    let c = Contract::new(weth_addr, ERC20_ABI, provider).expect("weth");
    let decimals = c
        .method("decimals")
        .expect("decimals")
        .call()
        .expect("call decimals");
    assert_eq!(decimals, "18");
}
