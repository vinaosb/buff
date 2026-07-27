//! ABI type encoding / decoding round-trip tests for `buff-web3`.
//!
//! These tests exercise the [`Token`] type (re-exported from
//! `ethers::abi::Token`) via the canonical ethers ABI codec
//! (`ethers::abi::encode` / `ethers::abi::decode`). This is the
//! exact code path that `ContractMethod::call` and `ContractMethod::send`
//! use internally to marshal method arguments and unmarshal return
//! values — so a round-trip here is a strong guarantee that the
//! ABI types the `buff-web3` surface exposes behave correctly.
//!
//! All tests are hermetic (no network, no live Ethereum node).
//! They depend only on the `ethers` ABI codec, which is a pure-Rust
//! arithmetic / byte-packing layer.
//!
//! # Coverage
//!
//! Each `Token` variant gets at least one round-trip test:
//!   - `Token::Uint`     (uint256, uint8, uint128)
//!   - `Token::Int`      (int256, int8 — positive and negative)
//!   - `Token::Address`  (zero address, vitalik address, checksummed)
//!   - `Token::Bool`     (true and false)
//!   - `Token::String`   (empty, ASCII, unicode)
//!   - `Token::Bytes`    (empty, short, long — dynamic-length)
//!   - `Token::FixedBytes` (bytes32 — the canonical Ethereum word)
//!   - `Token::Array`    (dynamic-length, mixed scalar elements)
//!   - `Token::FixedArray` (fixed-length)
//!   - `Token::Tuple`    (multi-field struct)
//!
//! Plus structural smoke tests for the `ContractMethod.arg` /
//! `.args` builder API (which feeds tokens into the encoder).

use buff_web3::{Contract, ContractMethod, Provider, Token};

use ethers::abi::{decode, encode, ParamType};
use ethers::types::{Address, U128, U256};

// Note: `Token::Int(U256)` represents a signed int256 via two's
// complement (the high bit means negative). For example, -1 in
// int256 is `U256::MAX` (all 1 bits). The codec preserves this
// representation through encode/decode round-trips.

// Well-known Ethereum constants used across the round-trip tests.
//
// `ZERO_ADDR` is the canonical burn/minter address (eip-170 "burn").
// `VITALIK_ADDR` is vitalik.eth's resolved address (well-known mainnet
// account; the hex form is stable since 2015).
//
// `ANVIL_KEY_A` is Hardhat / Anvil's first derived test key — used by
// `Wallet::default()` and documented at
// https://book.getfoundry.sh/reference/anvil/. NEVER use on mainnet.
const ZERO_ADDR: &str = "0x0000000000000000000000000000000000000000";
const VITALIK_ADDR: &str = "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045";

// A minimal ABI used for the ContractMethod builder smoke tests.
// `transfer(address,uint256)` is the canonical ERC-20 write method;
// `balanceOf(address)` is the canonical ERC-20 read method.
const ERC20_ABI: &str = r#"[
    {"type":"function","name":"balanceOf","inputs":[{"name":"account","type":"address"}],"outputs":[{"name":"","type":"uint256"}],"stateMutability":"view"},
    {"type":"function","name":"transfer","inputs":[{"name":"to","type":"address"},{"name":"amount","type":"uint256"}],"outputs":[{"name":"","type":"bool"}],"stateMutability":"nonpayable"}
]"#;

// ============================================================================
// Section 1 — Token::Uint round-trips (the most common ABI type).
// ============================================================================

#[test]
fn uint256_zero_round_trips() {
    let token = Token::Uint(U256::zero());
    let encoded = encode(std::slice::from_ref(&token));
    assert_eq!(encoded.len(), 32, "uint256 encodes to exactly 32 bytes");
    let decoded = decode(&[ParamType::Uint(256)], &encoded).expect("decode uint256 zero");
    assert_eq!(decoded.as_slice(), &[token]);
}

#[test]
fn uint256_small_value_round_trips() {
    let token = Token::Uint(U256::from(42u64));
    let encoded = encode(std::slice::from_ref(&token));
    let decoded = decode(&[ParamType::Uint(256)], &encoded).expect("decode uint256 42");
    assert_eq!(decoded.as_slice(), &[token]);
}

#[test]
fn uint256_max_u64_round_trips() {
    let token = Token::Uint(U256::from(u64::MAX));
    let encoded = encode(std::slice::from_ref(&token));
    let decoded = decode(&[ParamType::Uint(256)], &encoded).expect("decode uint256 max u64");
    assert_eq!(decoded.as_slice(), &[token]);
}

#[test]
fn uint256_large_value_round_trips() {
    // 2^128 — large enough to overflow u64 but well within uint256.
    let big = U256::from(1u64) << 128;
    let token = Token::Uint(big);
    let encoded = encode(std::slice::from_ref(&token));
    let decoded = decode(&[ParamType::Uint(256)], &encoded).expect("decode 2^128");
    assert_eq!(decoded.as_slice(), &[token]);
}

#[test]
fn uint256_max_round_trips() {
    // 2^256 - 1 — the maximum uint256 value (used for "unlimited"
    // allowance in ERC-20 approve).
    let max = U256::MAX;
    let token = Token::Uint(max);
    let encoded = encode(std::slice::from_ref(&token));
    let decoded = decode(&[ParamType::Uint(256)], &encoded).expect("decode uint256 max");
    assert_eq!(decoded.as_slice(), &[token]);
}

#[test]
fn uint128_round_trips_via_u128() {
    // Verify the same codec path used by Provider::get_balance
    // (which returns the low 128 bits of the U256 wei balance).
    let val = U128::from(1_000_000_000_000_000_000u128); // 1 ETH in wei
    let token = Token::Uint(val.into()); // U128 → U256 zero-extends
    let encoded = encode(std::slice::from_ref(&token));
    let decoded = decode(&[ParamType::Uint(128)], &encoded).expect("decode uint128");
    assert_eq!(decoded.as_slice(), &[token]);
}

#[test]
fn uint8_round_trips_for_erc20_decimals() {
    // uint8 is the type returned by ERC-20 `decimals()` (typically 18).
    let token = Token::Uint(U256::from(18u8));
    let encoded = encode(std::slice::from_ref(&token));
    let decoded = decode(&[ParamType::Uint(8)], &encoded).expect("decode uint8");
    assert_eq!(decoded.as_slice(), &[token]);
}

// ============================================================================
// Section 2 — Token::Int round-trips (signed; tests two's-complement).
//
// Note: `Token::Int` takes a `U256` whose bit pattern is interpreted
// as a signed two's-complement value. -1 in int256 = `U256::MAX`.
// ============================================================================

#[test]
fn int256_positive_round_trips() {
    let token = Token::Int(U256::from(123u64));
    let encoded = encode(std::slice::from_ref(&token));
    let decoded = decode(&[ParamType::Int(256)], &encoded).expect("decode positive int256");
    assert_eq!(decoded.as_slice(), &[token]);
}

#[test]
fn int256_negative_minus_one_round_trips() {
    // -1 in int256 = U256::MAX (all 1 bits) under two's complement.
    let token = Token::Int(U256::MAX);
    let encoded = encode(std::slice::from_ref(&token));
    assert!(
        encoded.iter().take(32).all(|b| *b == 0xff),
        "negative int256 (-1) should encode as all-0xff bytes (two's complement)"
    );
    let decoded = decode(&[ParamType::Int(256)], &encoded).expect("decode -1");
    assert_eq!(decoded.as_slice(), &[token]);
}

#[test]
fn int256_large_negative_round_trips() {
    // -1_000_000_000_000 in int256 = U256::MAX - 999_999_999_999 + 1
    // = (2^256) - 1_000_000_000_000.
    let val = U256::MAX - U256::from(1_000_000_000_000u64) + U256::one();
    let token = Token::Int(val);
    let encoded = encode(std::slice::from_ref(&token));
    assert!(
        encoded.iter().take(8).all(|b| *b == 0xff),
        "large negative int256 should have all-0xff high bytes"
    );
    let decoded = decode(&[ParamType::Int(256)], &encoded).expect("decode large negative");
    assert_eq!(decoded.as_slice(), &[token]);
}

// ============================================================================
// Section 3 — Token::Address round-trips.
// ============================================================================

#[test]
fn address_zero_round_trips() {
    let token = Token::Address(Address::zero());
    let encoded = encode(std::slice::from_ref(&token));
    assert_eq!(
        encoded.len(),
        32,
        "address is right-padded to 32 bytes in ABI encoding"
    );
    let decoded = decode(&[ParamType::Address], &encoded).expect("decode zero address");
    assert_eq!(decoded.as_slice(), &[token]);
}

#[test]
fn address_vitalik_round_trips() {
    let addr: Address = VITALIK_ADDR.parse().expect("vitalik address parses");
    let token = Token::Address(addr);
    let encoded = encode(std::slice::from_ref(&token));
    let decoded = decode(&[ParamType::Address], &encoded).expect("decode vitalik address");
    assert_eq!(decoded.as_slice(), &[token]);
}

#[test]
fn address_lowercase_and_checksummed_produce_same_token() {
    // EIP-55 checksummed form vs lowercase should produce identical
    // Address values (case-insensitive hex parsing).
    let lower: Address = VITALIK_ADDR.to_lowercase().parse().expect("lower");
    let checked: Address = VITALIK_ADDR.parse().expect("checked");
    assert_eq!(lower, checked, "case should not affect address equality");
    let t1 = Token::Address(lower);
    let t2 = Token::Address(checked);
    assert_eq!(encode(&[t1]), encode(&[t2]));
}

// ============================================================================
// Section 4 — Token::Bool round-trips.
// ============================================================================

#[test]
fn bool_true_round_trips() {
    let token = Token::Bool(true);
    let encoded = encode(std::slice::from_ref(&token));
    // ABI bool encodes as uint8 with value 0 or 1, right-padded to 32.
    assert_eq!(encoded[31], 1, "true should encode as 1 in the low byte");
    assert!(
        encoded.iter().take(31).all(|b| *b == 0),
        "high 31 bytes should be zero"
    );
    let decoded = decode(&[ParamType::Bool], &encoded).expect("decode true");
    assert_eq!(decoded.as_slice(), &[token]);
}

#[test]
fn bool_false_round_trips() {
    let token = Token::Bool(false);
    let encoded = encode(std::slice::from_ref(&token));
    assert!(
        encoded.iter().take(32).all(|b| *b == 0),
        "false should encode as all-zero word"
    );
    let decoded = decode(&[ParamType::Bool], &encoded).expect("decode false");
    assert_eq!(decoded.as_slice(), &[token]);
}

// ============================================================================
// Section 5 — Token::String round-trips (dynamic-length).
// ============================================================================

#[test]
fn string_empty_round_trips() {
    let token = Token::String(String::new());
    let encoded = encode(std::slice::from_ref(&token));
    let decoded = decode(&[ParamType::String], &encoded).expect("decode empty string");
    assert_eq!(decoded.as_slice(), &[token]);
}

#[test]
fn string_ascii_round_trips() {
    let token = Token::String("Hello, Ethereum!".into());
    let encoded = encode(std::slice::from_ref(&token));
    let decoded = decode(&[ParamType::String], &encoded).expect("decode ascii string");
    assert_eq!(decoded.as_slice(), &[token]);
}

#[test]
fn string_unicode_round_trips() {
    // Unicode exercises the dynamic-length encoding (length prefix +
    // utf-8 bytes + zero-padding to 32-byte boundary).
    let token = Token::String("Olá, Ethereum! 🦄".into());
    let encoded = encode(std::slice::from_ref(&token));
    let decoded = decode(&[ParamType::String], &encoded).expect("decode unicode string");
    assert_eq!(decoded.as_slice(), &[token]);
}

#[test]
fn string_longer_than_32_bytes_round_trips() {
    // 64-char string — forces multi-word encoding.
    let long = "a".repeat(64);
    let token = Token::String(long);
    let encoded = encode(std::slice::from_ref(&token));
    let decoded = decode(&[ParamType::String], &encoded).expect("decode long string");
    assert_eq!(decoded.as_slice(), &[token]);
}

// ============================================================================
// Section 6 — Token::Bytes round-trips (dynamic-length byte array).
// ============================================================================

#[test]
fn bytes_empty_round_trips() {
    let token = Token::Bytes(Vec::new());
    let encoded = encode(std::slice::from_ref(&token));
    let decoded = decode(&[ParamType::Bytes], &encoded).expect("decode empty bytes");
    assert_eq!(decoded.as_slice(), &[token]);
}

#[test]
fn bytes_short_round_trips() {
    let token = Token::Bytes(vec![0xde, 0xad, 0xbe, 0xef]);
    let encoded = encode(std::slice::from_ref(&token));
    let decoded = decode(&[ParamType::Bytes], &encoded).expect("decode short bytes");
    assert_eq!(decoded.as_slice(), &[token]);
}

#[test]
fn bytes_long_round_trips() {
    // 100 bytes — exercises multi-word encoding.
    let data: Vec<u8> = (0..100u8).collect();
    let token = Token::Bytes(data);
    let encoded = encode(std::slice::from_ref(&token));
    let decoded = decode(&[ParamType::Bytes], &encoded).expect("decode long bytes");
    assert_eq!(decoded.as_slice(), &[token]);
}

// ============================================================================
// Section 7 — Token::FixedBytes round-trips (bytes32, the Ethereum word).
// ============================================================================

#[test]
fn fixed_bytes_32_round_trips() {
    // bytes32 is the canonical "Ethereum word" — used for hashes,
    // storage slots, and immutable values.
    let mut data = [0u8; 32];
    for (i, b) in data.iter_mut().enumerate() {
        *b = i as u8;
    }
    let token = Token::FixedBytes(data.to_vec());
    let encoded = encode(std::slice::from_ref(&token));
    assert_eq!(encoded.len(), 32, "bytes32 encodes to exactly one word");
    let decoded = decode(&[ParamType::FixedBytes(32)], &encoded).expect("decode bytes32");
    assert_eq!(decoded.as_slice(), &[token]);
}

#[test]
fn fixed_bytes_zero_round_trips() {
    // All-zero bytes32 — used as the empty storage slot sentinel.
    let token = Token::FixedBytes(vec![0u8; 32]);
    let encoded = encode(std::slice::from_ref(&token));
    let decoded = decode(&[ParamType::FixedBytes(32)], &encoded).expect("decode zero bytes32");
    assert_eq!(decoded.as_slice(), &[token]);
}

// ============================================================================
// Section 8 — Token::Array round-trips (dynamic-length, uniform element).
// ============================================================================

#[test]
fn array_empty_uint_round_trips() {
    let token = Token::Array(Vec::new());
    let encoded = encode(std::slice::from_ref(&token));
    let decoded = decode(
        &[ParamType::Array(Box::new(ParamType::Uint(256)))],
        &encoded,
    )
    .expect("decode empty uint array");
    assert_eq!(decoded.as_slice(), &[token]);
}

#[test]
fn array_of_uints_round_trips() {
    let items = vec![
        Token::Uint(U256::from(1u64)),
        Token::Uint(U256::from(2u64)),
        Token::Uint(U256::from(3u64)),
    ];
    let token = Token::Array(items);
    let encoded = encode(std::slice::from_ref(&token));
    let decoded = decode(
        &[ParamType::Array(Box::new(ParamType::Uint(256)))],
        &encoded,
    )
    .expect("decode uint array");
    assert_eq!(decoded.as_slice(), &[token]);
}

#[test]
fn array_of_addresses_round_trips() {
    // Common pattern: list of recipient addresses for batch transfer.
    let addr1: Address = VITALIK_ADDR.parse().expect("vitalik");
    let addr2 = Address::zero();
    let addr3: Address = "0x1234567890abcdef1234567890abcdef12345678"
        .parse()
        .expect("addr3");
    let items = vec![
        Token::Address(addr1),
        Token::Address(addr2),
        Token::Address(addr3),
    ];
    let token = Token::Array(items);
    let encoded = encode(std::slice::from_ref(&token));
    let decoded = decode(&[ParamType::Array(Box::new(ParamType::Address))], &encoded)
        .expect("decode address array");
    assert_eq!(decoded.as_slice(), &[token]);
}

// ============================================================================
// Section 9 — Token::FixedArray round-trips (fixed-length).
// ============================================================================

#[test]
fn fixed_array_of_two_uints_round_trips() {
    // Common pattern: 2-element fixed array (e.g., price feeds).
    let items = vec![
        Token::Uint(U256::from(100u64)),
        Token::Uint(U256::from(200u64)),
    ];
    let token = Token::FixedArray(items);
    let encoded = encode(std::slice::from_ref(&token));
    let decoded = decode(
        &[ParamType::FixedArray(Box::new(ParamType::Uint(256)), 2)],
        &encoded,
    )
    .expect("decode fixed array");
    assert_eq!(decoded.as_slice(), &[token]);
}

// ============================================================================
// Section 10 — Token::Tuple round-trips (multi-field struct).
// ============================================================================

#[test]
fn tuple_address_uint_round_trips() {
    // Common pattern: (address, uint256) — e.g., a credit entry.
    let addr: Address = VITALIK_ADDR.parse().expect("vitalik");
    let items = vec![
        Token::Address(addr),
        Token::Uint(U256::from(1_000_000_000_000_000_000u128)), // 1 ETH
    ];
    let token = Token::Tuple(items);
    let encoded = encode(std::slice::from_ref(&token));
    let decoded = decode(
        &[ParamType::Tuple(vec![
            ParamType::Address,
            ParamType::Uint(256),
        ])],
        &encoded,
    )
    .expect("decode address+uint tuple");
    assert_eq!(decoded.as_slice(), &[token]);
}

#[test]
fn tuple_nested_with_array_round_trips() {
    // Complex pattern: (uint256, address[]) — e.g., a threshold + voters.
    let inner_items = vec![
        Token::Address(Address::zero()),
        Token::Address(VITALIK_ADDR.parse().expect("vitalik")),
    ];
    let items = vec![Token::Uint(U256::from(2u64)), Token::Array(inner_items)];
    let token = Token::Tuple(items);
    let encoded = encode(std::slice::from_ref(&token));
    let decoded = decode(
        &[ParamType::Tuple(vec![
            ParamType::Uint(256),
            ParamType::Array(Box::new(ParamType::Address)),
        ])],
        &encoded,
    )
    .expect("decode nested tuple");
    assert_eq!(decoded.as_slice(), &[token]);
}

// ============================================================================
// Section 11 — Multi-argument encoding (the ContractMethod args list).
// ============================================================================

#[test]
fn multi_argument_uint_address_round_trips() {
    // Two-top-level-argument encoding: transfer(address,uint256) is
    // the canonical ERC-20 method that ContractMethod builds. This
    // verifies the encoder handles the 2-arg case correctly.
    let addr: Address = VITALIK_ADDR.parse().expect("vitalik");
    let tokens = vec![
        Token::Address(addr),
        Token::Uint(U256::from(1_000_000_000_000_000_000u128)),
    ];
    let encoded = encode(&tokens);
    let decoded = decode(&[ParamType::Address, ParamType::Uint(256)], &encoded)
        .expect("decode 2-arg encoding");
    assert_eq!(decoded, tokens);
}

#[test]
fn multi_argument_three_values_round_trips() {
    let tokens = vec![
        Token::Address(Address::zero()),
        Token::Address(VITALIK_ADDR.parse().expect("vitalik")),
        Token::Uint(U256::from(123u64)),
    ];
    let encoded = encode(&tokens);
    let decoded = decode(
        &[ParamType::Address, ParamType::Address, ParamType::Uint(256)],
        &encoded,
    )
    .expect("decode 3-arg encoding");
    assert_eq!(decoded, tokens);
}

// ============================================================================
// Section 12 — ContractMethod.arg / args builder smoke tests.
//
// These verify the public builder API accepts every Token variant
// without panic. They do NOT exercise the wire-level encoding (that
// happens inside `call()` / `send()` which require network). The
// builder just stores tokens for later encoding — the round-trip
// tests above cover the encoding correctness directly.
// ============================================================================

fn make_read_only_contract() -> Contract {
    let provider = Provider::new("http://localhost:8545").expect("provider");
    Contract::new(ZERO_ADDR, ERC20_ABI, provider).expect("contract parses ERC-20 ABI")
}

#[test]
fn contract_method_arg_accepts_address_token() {
    let c = make_read_only_contract();
    let method: ContractMethod = c.method("transfer").expect("transfer exists");
    let addr: Address = VITALIK_ADDR.parse().expect("vitalik");
    // Should not panic — Token::Address is a valid ABI argument.
    let _chained = method.arg(Token::Address(addr));
}

#[test]
fn contract_method_arg_accepts_uint_token() {
    let c = make_read_only_contract();
    let method = c.method("transfer").expect("transfer exists");
    let _chained = method.arg(Token::Uint(U256::from(1u64)));
}

#[test]
fn contract_method_arg_chain_two_args() {
    let c = make_read_only_contract();
    let method = c.method("transfer").expect("transfer exists");
    let addr: Address = VITALIK_ADDR.parse().expect("vitalik");
    // Verify the builder is chainable: arg().arg() compiles and runs.
    let _chained = method
        .arg(Token::Address(addr))
        .arg(Token::Uint(U256::from(1u64)));
}

#[test]
fn contract_method_args_bulk_add_from_iterator() {
    let c = make_read_only_contract();
    let method = c.method("transfer").expect("transfer exists");
    let addr: Address = VITALIK_ADDR.parse().expect("vitalik");
    let bulk = vec![Token::Address(addr), Token::Uint(U256::from(42u64))];
    // The `args` method accepts any IntoIterator<Item = Token>.
    let _chained = method.args(bulk);
}

#[test]
fn contract_method_args_accepts_empty_iterator() {
    let c = make_read_only_contract();
    let method = c.method("balanceOf").expect("balanceOf exists");
    // Empty iterator is a no-op — should not panic.
    let _chained = method.args(std::iter::empty());
}

#[test]
fn contract_method_arg_accepts_bool_token() {
    // Bool tokens are valid ABI args (e.g., for `approve` return value).
    let c = make_read_only_contract();
    let method = c.method("transfer").expect("transfer exists");
    let _chained = method.arg(Token::Bool(true));
}

#[test]
fn contract_method_arg_accepts_string_token() {
    let c = make_read_only_contract();
    let method = c.method("transfer").expect("transfer exists");
    let _chained = method.arg(Token::String("hello".into()));
}

#[test]
fn contract_method_arg_accepts_bytes_token() {
    let c = make_read_only_contract();
    let method = c.method("transfer").expect("transfer exists");
    let _chained = method.arg(Token::Bytes(vec![0xde, 0xad, 0xbe, 0xef]));
}

#[test]
fn contract_method_arg_accepts_fixed_bytes_token() {
    let c = make_read_only_contract();
    let method = c.method("transfer").expect("transfer exists");
    let _chained = method.arg(Token::FixedBytes(vec![0u8; 32]));
}

#[test]
fn contract_method_arg_accepts_int_token() {
    let c = make_read_only_contract();
    let method = c.method("transfer").expect("transfer exists");
    // Token::Int takes U256 in two's-complement form (see Section 2 header).
    let _chained = method.arg(Token::Int(U256::MAX)); // -1 in int256
}

#[test]
fn contract_method_arg_accepts_array_token() {
    let c = make_read_only_contract();
    let method = c.method("transfer").expect("transfer exists");
    let inner = vec![Token::Uint(U256::from(1u64)), Token::Uint(U256::from(2u64))];
    let _chained = method.arg(Token::Array(inner));
}

#[test]
fn contract_method_arg_accepts_tuple_token() {
    let c = make_read_only_contract();
    let method = c.method("transfer").expect("transfer exists");
    let inner = vec![
        Token::Address(Address::zero()),
        Token::Uint(U256::from(1u64)),
    ];
    let _chained = method.arg(Token::Tuple(inner));
}
