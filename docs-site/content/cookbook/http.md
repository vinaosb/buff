+++
title = "HTTP"
weight = 41
+++

# HTTP recipes

Recipes for outbound HTTP using the `HttpClient` prelude type (T33,
v1.13 frameworks wave 5). `HttpClient` wraps `reqwest` behind a safe
FFI boundary; the surface mirrors `reqwest`'s builder chain.

> **Status:** `HttpClient` carries an EXPERIMENTAL stability badge
> through v1.18. The codegen lowering lands end-to-end with the
> Cargo-project pipeline (T120 / v1.3); `buff check` validates the
> syntax today.

## Make a GET request

**Problem**: Fetch the body of a URL as text.

**Solution**:

```buff
func fetch_text(url: String) -> Result<String, Error>:
    let client = HttpClient.new()
    let resp = client.get(url).send()?
    return Ok(resp.text())

func main():
    match fetch_text("https://example.com") {
        Ok(body) => print(body),
        Err(e)   => print("fetch failed: " + e.string()),
    }
```

**Explanation**:

`HttpClient.new()` constructs a client with default settings (no
timeout, rustls backend, no redirect policy beyond the reqwest default).
`client.get(url)` returns a `RequestBuilder` — a chainable builder —
and `.send()` issues the request and returns a `Response`. `resp.text()`
consumes the response body and decodes it as UTF-8.

The whole chain is fallible: `send()` returns `Result<Response,
HttpError>`, so we propagate it with `?`. The caller matches both arms
of the `Result` — Buff's `match` is exhaustive, so forgetting the
`Err` branch is a compile error, not a runtime surprise.

## Parse a JSON response

**Problem**: Fetch JSON and turn it into a Buff `Map`.

**Solution**:

```buff
func fetch_user(url: String) -> Result<Map<String, String>, Error>:
    let client = HttpClient.new()
    let resp = client.get(url).send()?
    let body = resp.json()?
    return Ok(body)

func main():
    match fetch_user("https://api.example.com/users/1") {
        Ok(user) => print(user.get("name")),
        Err(e)   => print("error: " + e.string()),
    }
```

**Explanation**:

`Response.json()` parses the body as JSON and returns a
`serde_json::Value`-shaped `Map<String, String>` at the Buff layer —
heterogeneous values are surfaced as `Map<String, String>` for now,
matching how the `Toml.parse` and `Yaml.parse` prelude modules
surface parsed data. A future `JsonValue` prelude type may add typed
accessors (`obj.get_array("items")`, `obj.get_int("count")`).

For typed JSON, the recommended pattern today is to fetch as text and
re-parse with the structure-aware prelude module that matches your
wire format — `Toml.parse(text)`, `Yaml.parse(text)`, or `Csv.parse(text)`.

## POST a form

**Problem**: Send `application/x-www-form-urlencoded` data via POST.

**Solution**:

```buff
func login(email: String, password: String) -> Result<String, Error>:
    let form = {
        "email":    email,
        "password": password,
    }
    let resp = HttpClient.new()
        .post("https://api.example.com/login")
        .json(form)
        .send()?
    return Ok(resp.text())

func main():
    match login("user@example.com", "hunter2") {
        Ok(body)  => print(body),
        Err(e)    => print("login failed: " + e.string()),
    }
```

**Explanation**:

The map literal `{ "email": ..., "password": ... }` is a `Map<String,
String>`; passing it to `.json(body)` serialises it (via `serde_json`)
and sets `Content-Type: application/json` automatically. For true
`x-www-form-urlencoded`, the planned `.form(body)` builder is on the
v1.18+ roadmap — until then, `.json()` is the canonical shape.

`.post(url)` returns the same `RequestBuilder` type `.get(url)` does,
so the chain `.post(url).json(body).send()` mirrors the GET case. Any
call site that does both GET and POST can factor the common tail
(`resp.text()`, `resp.json()`) into a helper.

## Set custom headers

**Problem**: Send Authorization + Accept headers with the request.

**Solution**:

```buff
func fetch_protected(token: String, url: String) -> Result<String, Error>:
    let resp = HttpClient.new()
        .get(url)
        .header(name: "Authorization", value: "Bearer " + token)
        .header(name: "Accept", value: "application/json")
        .send()?
    return Ok(resp.text())
```

**Explanation**:

`RequestBuilder.header(name, value)` appends a header — call it
multiple times to set several. The arguments are named (`name:`,
`value:`) per Buff's named-argument rule for multi-arg calls; positional
works too, but the explicit form reads more clearly when you copy the
recipe.

Headers are case-insensitive at the HTTP layer, so `"Authorization"`
and `"authorization"` are the same header. The `reqwest` crate Buff
wraps normalises the case internally; you don't need to.

## Retry with exponential backoff

**Problem**: Retry a flaky request up to N times with growing delay.

**Solution**:

```buff
func fetch_with_retry(url: String, max_tries: Int) -> Result<String, Error>:
    var attempt: Int = 0
    var last_err: Error = Error("no attempts made")
    while attempt < max_tries:
        let client = HttpClient.new()
        let result = client.get(url).send()
        match result {
            Ok(resp) => return Ok(resp.text()),
            Err(e)   => last_err = e
        }
        attempt = attempt + 1
        sleep(Duration.seconds(pow(2, attempt)))
    return last_err

func main():
    match fetch_with_retry("https://flaky.example.com", max_tries: 4) {
        Ok(body) => print(body),
        Err(e)   => print("gave up: " + e.string()),
    }
```

**Explanation**:

The loop tries the request, returning early on success. On failure it
records the error and sleeps for `2 ** attempt` seconds — 2s, 4s, 8s,
16s — before the next try. The total wall-clock budget for 4 attempts
is ~30s, which is a sane default for transient network errors.

`buff-resilience` (T36) ships a production-grade `Retry` type with
jitter, circuit-breaker integration, and per-attempt timeouts. This
recipe is the 15-line version; for production code, prefer
`buff-resilience::Retry.with_policy(...)`.

## Inspect response status and headers

**Problem**: Branch on the HTTP status code and read a response header.

**Solution**:

```buff
func check_health(url: String) -> Bool:
    let resp = HttpClient.new().get(url).send()
    match resp {
        Ok(r) =>
            if r.status() == 200:
                print(r.header("X-Version"))
                return true
            else:
                return false
        Err(_) => return false
    }
```

**Explanation**:

`Response.status()` returns the numeric HTTP status (e.g. `200`,
`404`). `Response.header(name)` returns `Option<String>` — `None`
when the header is absent, never a panic. Matching on the `Result`
first keeps the error path explicit; the success arm then branches on
the status code with a plain `if`/`else`.

For richer dispatch (different handlers per status family), factor
each arm into a helper and chain them through the `|>` pipeline
operator:

```buff
resp |> on_ok() |> on_redirect() |> on_error()
```

`|>` is a parse-time desugar to nested function calls — no new AST
nodes, no runtime cost.
