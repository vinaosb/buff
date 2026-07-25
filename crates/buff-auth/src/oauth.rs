//! OAuth2 authorization-code flow client (pure-Rust, rustls-tls — NO native-tls, NO `ring`).

use std::panic::{catch_unwind, AssertUnwindSafe};

// `TokenResponse` is no longer imported: oauth2 4.4 dropped the
// `BasicTokenFields` / `BasicTokenExtraFields` exports in favour of
// the `BasicClient` / `BasicTokenResponse` type aliases (which thread
// `EmptyExtraTokenFields` through the new 6-generic `Client`).
use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, PkceCodeChallenge, RedirectUrl,
    Scope, TokenUrl,
};
use serde_json::{Map, Value};

use crate::error::AuthError;

/// Bundled inputs needed to construct an OAuth2 auth-code client.
///
/// All fields are owned `String` so the struct is `Send + 'static` per
/// FFI guide R4. `client_secret` is optional — public clients (mobile /
/// SPA) use PKCE and have no secret.
#[derive(Debug, Clone)]
pub struct OAuth2Client {
    client_id: String,
    client_secret: Option<String>,
    auth_url: String,
    token_url: String,
    redirect_url: String,
    scopes: Vec<String>,
}

impl OAuth2Client {
    /// Build a new OAuth2 client. `client_secret = None` triggers the
    /// PKCE flow (public client). Wraps nothing externally — just owned
    /// storage.
    pub fn new(
        client_id: String,
        client_secret: Option<String>,
        auth_url: String,
        token_url: String,
        redirect_url: String,
        scopes: Vec<String>,
    ) -> Self {
        OAuth2Client {
            client_id,
            client_secret,
            auth_url,
            token_url,
            redirect_url,
            scopes,
        }
    }

    fn build_core(&self) -> Result<oauth2::basic::BasicClient, AuthError> {
        let client_id = ClientId::new(self.client_id.clone());
        let auth_url = AuthUrl::new(self.auth_url.clone())
            .map_err(|e| AuthError::OAuth2(format!("auth url: {e}")))?;
        let token_url = TokenUrl::new(self.token_url.clone())
            .map_err(|e| AuthError::OAuth2(format!("token url: {e}")))?;
        let redirect_url = RedirectUrl::new(self.redirect_url.clone())
            .map_err(|e| AuthError::OAuth2(format!("redirect url: {e}")))?;
        // oauth2 4.4 moved `client_secret` into `Client::new` (was a
        // separate `set_client_secret` builder step). The 4-arg form is
        // `(client_id, Option<ClientSecret>, auth_url, Option<TokenUrl>)`.
        let secret = self
            .client_secret
            .as_ref()
            .map(|s| ClientSecret::new(s.clone()));
        let builder = oauth2::Client::new(client_id, secret, auth_url, Some(token_url))
            .set_redirect_uri(redirect_url);
        Ok(builder)
    }

    /// Build the authorization URL the user must visit in a browser.
    ///
    /// Returns the URL as a String. If the client has no secret (PKCE
    /// public client), a fresh PKCE challenge is generated and embedded
    /// in the URL; the matching verifier is appended to the URL as a
    /// fragment (`#pkce_verifier=...`) for the caller to extract and
    /// pass back to `exchange_code`. Wraps
    /// `oauth2::Client::authorize_url(...)` in `catch_unwind` per FFI
    /// guide R6.
    pub fn authorization_url(&self) -> Result<String, AuthError> {
        let client = self.clone();
        let result = catch_unwind(AssertUnwindSafe(|| -> Result<String, AuthError> {
            let core = client.build_core()?;
            let (pkce_verifier, auth_url) = if client.client_secret.is_none() {
                let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
                let mut req = core
                    .authorize_url(|| CsrfToken::new_random())
                    .set_pkce_challenge(challenge);
                for scope in &client.scopes {
                    req = req.add_scope(Scope::new(scope.clone()));
                }
                let url = req.url();
                (Some(verifier), url.0.to_string())
            } else {
                let mut req = core.authorize_url(|| CsrfToken::new_random());
                for scope in &client.scopes {
                    req = req.add_scope(Scope::new(scope.clone()));
                }
                let (url, _) = req.url();
                (None, url.to_string())
            };
            match pkce_verifier {
                Some(v) => Ok(format!("{auth_url}#pkce_verifier={}", v.secret())),
                None => Ok(auth_url),
            }
        }));
        match result {
            Ok(Ok(url)) => Ok(url),
            Ok(Err(err)) => Err(err),
            Err(_) => Err(AuthError::Panic),
        }
    }

    /// Exchange an authorization code for an access token.
    ///
    /// Performs a blocking HTTP POST to the configured token endpoint
    /// via `reqwest::blocking` (rustls-tls). `pkce_verifier` is the
    /// PKCE code verifier extracted from the auth URL fragment (None
    /// for confidential clients with a secret). Returns the token
    /// response as a JSON-shaped map (typically carrying
    /// `access_token`, `token_type`, `expires_in`, `refresh_token`,
    /// `scope`, and provider-specific fields). Wraps
    /// `oauth2::Client::exchange_code(...)` in `catch_unwind` per FFI
    /// guide R6.
    pub fn exchange_code(
        &self,
        code: &str,
        pkce_verifier: Option<&str>,
    ) -> Result<Map<String, Value>, AuthError> {
        let client = self.clone();
        let code_owned = code.to_string();
        let verifier_owned = pkce_verifier.map(|s| s.to_string());
        let result = catch_unwind(AssertUnwindSafe(
            || -> Result<Map<String, Value>, AuthError> {
                let core = client.build_core()?;
                let http = reqwest::blocking::Client::builder()
                    .use_rustls_tls()
                    .build()
                    .map_err(|e| AuthError::OAuth2(format!("http client: {e}")))?;
                let mut req = core.exchange_code(AuthorizationCode::new(code_owned));
                if let Some(verifier) = verifier_owned.as_ref() {
                    req = req.set_pkce_verifier(oauth2::PkceCodeVerifier::new(verifier.clone()));
                }
                // oauth2 4.4 takes a `FnOnce(HttpRequest) -> Result<HttpResponse, RE>`
                // closure instead of an `&dyn HttpClient` reference. The closure
                // converts `http::Request<Vec<u8>>` to a blocking reqwest call and
                // back to `http::Response<Vec<u8>>` (reqwest re-exports the
                // underlying `http` types so Method / StatusCode / HeaderMap
                // line up).
                let token_response =
                    req.request(move |http_req| execute_http_request(&http, http_req))?;
                let json = serde_json::to_value(token_response)?;
                match json {
                    Value::Object(map) => Ok(map),
                    other => {
                        let mut single = Map::new();
                        single.insert("value".to_string(), other);
                        Ok(single)
                    }
                }
            },
        ));
        match result {
            Ok(Ok(map)) => Ok(map),
            Ok(Err(err)) => Err(err),
            Err(_) => Err(AuthError::Panic),
        }
    }
}

// ---- internal helpers ----------------------------------------------------

/// Convert an `oauth2::HttpRequest` into a blocking reqwest call,
/// returning an `oauth2::HttpResponse`. Required by oauth2 4.4's
/// `CodeTokenRequest::request<F, RE>(.., http_client: F)` signature
/// (was previously a `&dyn HttpClient` trait object in 4.x).
///
/// **http-version bridge**: oauth2 4.4 pins `http` 0.2 while the
/// workspace's `reqwest` 0.12 uses `http` 1.x. The two versions' types
/// (Method / StatusCode / HeaderMap / HeaderName / HeaderValue) are
/// NOT interchangeable, so we round-trip each field through its byte
/// representation. `url::Url` is shared between the two crates so the
/// URL passes through unchanged.
fn execute_http_request(
    client: &reqwest::blocking::Client,
    http_req: oauth2::HttpRequest,
) -> Result<oauth2::HttpResponse, AuthError> {
    // oauth2::http::Method (http 0.2) -> reqwest::Method (http 1.x).
    let method = reqwest::Method::from_bytes(http_req.method.as_str().as_bytes())
        .map_err(|e| AuthError::OAuth2(format!("http method: {e}")))?;
    let mut req_builder = client.request(method, http_req.url);
    // Headers: http 0.2 -> http 1.x by way of &str / &[u8].
    for (name, value) in &http_req.headers {
        req_builder = req_builder.header(name.as_str(), value.as_bytes());
    }
    let response = req_builder
        .body(http_req.body)
        .send()
        .map_err(|e| AuthError::OAuth2(format!("http send: {e}")))?;

    // reqwest::StatusCode (http 1.x) -> oauth2::http::StatusCode (http 0.2).
    let status_code = oauth2::http::StatusCode::from_u16(response.status().as_u16())
        .map_err(|e| AuthError::OAuth2(format!("http status: {e}")))?;
    // Headers: http 1.x -> http 0.2 by way of bytes (name & value).
    let mut headers = oauth2::http::HeaderMap::new();
    for (name, value) in response.headers() {
        let header_name = oauth2::http::HeaderName::from_bytes(name.as_str().as_bytes())
            .map_err(|e| AuthError::OAuth2(format!("http header name: {e}")))?;
        let header_value = oauth2::http::HeaderValue::from_bytes(value.as_bytes())
            .map_err(|e| AuthError::OAuth2(format!("http header value: {e}")))?;
        headers.append(header_name, header_value);
    }
    let body = response
        .bytes()
        .map_err(|e| AuthError::OAuth2(format!("http body: {e}")))?
        .to_vec();
    Ok(oauth2::HttpResponse {
        status_code,
        headers,
        body,
    })
}

#[cfg(test)]
mod smoke_tests {
    use super::*;

    fn dummy() -> OAuth2Client {
        OAuth2Client::new(
            "client-1".to_string(),
            Some("secret".to_string()),
            "https://example.com/auth".to_string(),
            "https://example.com/token".to_string(),
            "https://example.com/cb".to_string(),
            vec![],
        )
    }

    #[test]
    fn authorization_url_for_confidential_client() {
        let c = dummy();
        let url = c.authorization_url().expect("auth url");
        assert!(url.contains("https://example.com/auth"), "url was: {url}");
        assert!(url.contains("client_id=client-1"));
        assert!(url.contains("redirect_uri="));
        assert!(
            !url.contains("#pkce_verifier="),
            "confidential client must NOT embed PKCE verifier"
        );
    }

    #[test]
    fn authorization_url_for_public_client_uses_pkce() {
        let c = OAuth2Client::new(
            "pub-1".to_string(),
            None,
            "https://example.com/auth".to_string(),
            "https://example.com/token".to_string(),
            "https://example.com/cb".to_string(),
            vec![],
        );
        let url = c.authorization_url().expect("auth url");
        assert!(url.contains("#pkce_verifier="), "url was: {url}");
    }

    #[test]
    fn authorization_url_rejects_bad_redirect_url() {
        let c = OAuth2Client::new(
            "x".to_string(),
            None,
            "https://example.com/auth".to_string(),
            "https://example.com/token".to_string(),
            "not-a-url".to_string(),
            vec![],
        );
        let err = c.authorization_url().unwrap_err();
        assert!(matches!(err, AuthError::OAuth2(_)));
    }
}
