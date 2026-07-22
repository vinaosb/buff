//! Integration tests for the `buff-auth` OAuth2 module.
//!
//! All tests are NO-NETWORK (only URL construction is exercised —
//! `exchange_code` against a real provider is deferred to integration
//! test infrastructure per the T34 task spec line 3302). 4 tests
//! (counted toward the T34 acceptance 15 tests).

use buff_auth::{AuthError, OAuth2Client};

fn confidential() -> OAuth2Client {
    OAuth2Client::new(
        "client-1".to_string(),
        Some("secret".to_string()),
        "https://accounts.example.com/auth".to_string(),
        "https://accounts.example.com/token".to_string(),
        "https://app.example.com/cb".to_string(),
        vec!["profile".to_string(), "email".to_string()],
    )
}

#[test]
fn oauth2_authorization_url_confidential_has_required_components() {
    let url = confidential().authorization_url().expect("auth url");
    assert!(url.starts_with("https://accounts.example.com/auth"), "url was: {url}");
    assert!(url.contains("client_id=client-1"), "missing client_id");
    assert!(url.contains("redirect_uri="), "missing redirect_uri");
    assert!(url.contains("response_type=code"), "missing response_type");
    assert!(
        !url.contains("#pkce_verifier="),
        "confidential client must NOT embed PKCE verifier"
    );
}

#[test]
fn oauth2_authorization_url_public_client_uses_pkce() {
    let public = OAuth2Client::new(
        "mobile-app".to_string(),
        None,
        "https://accounts.example.com/auth".to_string(),
        "https://accounts.example.com/token".to_string(),
        "myapp://callback".to_string(),
        vec![],
    );
    let url = public.authorization_url().expect("auth url");
    assert!(url.contains("#pkce_verifier="), "url was: {url}");
}

#[test]
fn oauth2_authorization_url_rejects_bad_redirect_url() {
    let bad = OAuth2Client::new(
        "x".to_string(),
        None,
        "https://accounts.example.com/auth".to_string(),
        "https://accounts.example.com/token".to_string(),
        "not a url".to_string(),
        vec![],
    );
    let err = bad.authorization_url().unwrap_err();
    assert!(matches!(err, AuthError::OAuth2(_)));
}

#[test]
fn oauth2_authorization_url_rejects_bad_auth_url() {
    let bad = OAuth2Client::new(
        "x".to_string(),
        None,
        "not a url".to_string(),
        "https://accounts.example.com/token".to_string(),
        "https://app.example.com/cb".to_string(),
        vec![],
    );
    let err = bad.authorization_url().unwrap_err();
    assert!(matches!(err, AuthError::OAuth2(_)));
}
