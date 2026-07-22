// T34 example: OAuth2 authorization-code flow — build the auth URL.
//
// The MVP shows URL construction for both a confidential client
// (with secret) and a public PKCE client. The actual code exchange
// requires a running OAuth2 provider, which is out of scope for this
// example (see tests/oauth2.rs for a no-network exchange shape test).

use buff_auth::OAuth2Client;

fn main() {
    let confidential = OAuth2Client::new(
        "my-client-id".to_string(),
        Some("my-client-secret".to_string()),
        "https://accounts.example.com/oauth2/auth".to_string(),
        "https://accounts.example.com/oauth2/token".to_string(),
        "https://myapp.example.com/callback".to_string(),
        vec!["profile".to_string(), "email".to_string()],
    );
    let url = confidential.authorization_url().expect("auth url");
    println!("confidential client auth URL:\n{url}\n");

    let public = OAuth2Client::new(
        "my-mobile-app".to_string(),
        None,
        "https://accounts.example.com/oauth2/auth".to_string(),
        "https://accounts.example.com/oauth2/token".to_string(),
        "myapp://callback".to_string(),
        vec!["profile".to_string()],
    );
    let url = public.authorization_url().expect("auth url");
    println!("public (PKCE) client auth URL:\n{url}");
}
