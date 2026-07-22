// T34 example: password hash + verify roundtrip.

use buff_auth::{password_hash, password_verify};

fn main() {
    let plain = "correct horse battery staple";
    let hash = password_hash(plain).expect("hash");
    println!("hash: {hash}");

    let ok = password_verify(plain, &hash).expect("verify shape");
    println!("verify correct: {ok}");

    let wrong = password_verify("not the password", &hash).expect("verify shape");
    println!("verify wrong: {wrong} (expected false)");
}
