# Chapter 3 — Build an API Server

In this chapter you'll build an HTTP JSON API in Buff. You'll learn:

- the **`buff-web`** framework crate and its `Web` / `Request` / `Response`
  types,
- how to register GET / POST / PUT / DELETE / PATCH routes,
- how to read JSON request bodies and write JSON responses,
- how middleware composes,
- why your handlers are *synchronous Buff functions* even though the server is
  async under the hood (Buff has no `await` keyword),
- how `Response.json(...)` and the prelude `Json.parse` / `Json.stringify`
  round-trip data.

The example you'll build is a small "todo" API: list, create, fetch, and delete
items. It demonstrates every HTTP verb Buff's web framework exposes.

## 3.1 Why `buff-web` exists

Rust already has a canonical web framework — [`axum`](https://crates.io/crates/axum)
0.8. Buff does not reinvent it. Instead, `buff-web` (shipped in v1.15, T17) is
a *safe wrapper* over axum + tokio + serde_json that follows Buff's
[FFI safety guide](https://github.com/buff-lang/buff/tree/v1x-frameworks/crates/buff-lang-ffi-guide/GUIDE.md).
Your Buff program talks to a small, panic-free surface; the wrapper translates
each call into the corresponding axum / tokio / serde operation.

This is the same pattern as [`buff-cli`](./chapter-2.md) wrapping `clap`, and
it's the pattern every framework crate in the Buff SDK follows: *find the
canonical Rust crate, wrap it safely, expose a Buff-shaped surface*. Buff never
competes with the Rust ecosystem — it rides on top of it.

## 3.2 Hello, web 🔶

> 🔶 The `buff-web` Rust crate is shipped (v1.15) with full test coverage. The
> Buff-side surface (`Web.new()` in `.buff` source) is a forward-declaration:
> the prelude-type + codegen lowering arm is a coordinated sibling task. The
> snippets below are valid Buff syntax matching the Rust examples in
> `crates/buff-web/examples/`. See
> [`crates/buff-web/AGENTS.md`](../../crates/buff-web/AGENTS.md) §"RELATIONSHIP
> TO OTHER CRATES" for the wiring map.

From [`crates/buff-web/examples/hello_web.buff`](../../crates/buff-web/examples/hello_web.buff):

```buff
from "buff/web" import Web, Request, Response

func main():
    app = Web.new()

    app.get(path: "/", handler: { _req => Response.text("hello, web") })
    app.get(path: "/health", handler: { _req => Response.json({ "status": "ok" }) })

    print("buff-web hello example listening on http://0.0.0.0:8080")
    app.listen(port: 8080)
```

Run it (once the wiring lands):

```bash
buff run hello_web.buff
```

Then in another terminal:

```bash
curl http://localhost:8080/
# hello, web
curl http://localhost:8080/health
# {"status":"ok"}
```

Let's unpack the new ideas.

### Imports

```buff
from "buff/web" import Web, Request, Response
```

Buff has two import forms:

- `import { name } from "./local.buff"` — a local module (relative path).
- `from "buff/web" import Web, Request, Response` — a framework crate
  (workspace path). The `"buff/web"` resolves to the `buff-web` crate's public
  surface.

The `from "..." import ...` form is for *namespace* imports (types you'll use
as `Web.method()`); the `import { ... } from "..."` form is for *value*
imports (functions you'll call bare). Both are covered in [Chapter 6 §6.7](./chapter-6.md).

### The `Web` builder

- `Web.new()` — returns an empty server (no bind address yet).
- `app.get(path: ..., handler: ...)` — register a GET route. The `handler` is
  a lambda `{ req => Response... }` taking the `Request` and returning a
  `Response`.
- `app.listen(port: 8080)` — bind `0.0.0.0:8080` and serve forever. This call
  *blocks* the calling thread.

The five routing methods mirror the five canonical HTTP verbs:
`app.get`, `app.post`, `app.put`, `app.delete`, `app.patch`. Exotic verbs
(HEAD, OPTIONS, TRACE, CONNECT) are intentionally out of scope for the MVP.

### Why the handler is synchronous

Notice there's no `async` keyword on the handler lambda and no `await` inside
it. Buff has **no `await` keyword at all**. The async runtime (tokio) lives
*inside* the `buff-web` wrapper; your handler is a plain synchronous closure
from the Buff side. This is one of Buff's signature ergonomics wins — covered
in depth in [Chapter 6 §6.8](./chapter-6.md).

The wrapper builds a fresh tokio `Runtime` per `listen` / `run` call and blocks
the calling thread on it, exactly as the FFI guide's "Example 3" pattern
prescribes. Your Buff code never knows async exists.

## 3.3 Reading the request

A handler receives a `Request`. The five accessors:

| Method | Returns | Notes |
|---|---|---|
| `req.method()` | `String` | `"GET"`, `"POST"`, ... |
| `req.path()` | `String` | URL path component (e.g. `/users/42`) |
| `req.header(name)` | `Option<String>` | case-insensitive header lookup |
| `req.body()` | `Result<String, Error>` | UTF-8 body |
| `req.json()` | `Result<Map<String, Unknown>, Error>` | parsed JSON body |

A handler that echoes back what it received:

```buff
from "buff/web" import Web, Request, Response

func main():
    app = Web.new()
    app.get(path: "/echo", handler: { req =>
        let ua = req.header("User-Agent").or(default: "unknown")
        Response.text("you hit {req.path()} via {req.method()} with UA {ua}")
    })
    app.listen(port: 8080)
```

### Path parameters

The MVP exposes the full URL path via `req.path()`. Extracting `{id}` segments
is the user's job with string ops for now:

```buff
app.get(path: "/users/{id}", handler: { req =>
    let path = req.path()                  // "/users/42"
    let id = path.split("/")[2]            // "42"
    Response.text("user id: {id}")
})
```

A future `req.param("id")` accessor that threads axum's path-match captures
through is planned for v1.18+.

## 3.4 Building responses

`Response` is a chainable builder. Three constructors cover most needs:

| Constructor | Produces |
|---|---|
| `Response.text(s)` | `200 text/plain; charset=utf-8` |
| `Response.json(value)` | `200 application/json` |
| `Response.status_only(code)` | empty body, no Content-Type |

Then chain mutators:

```buff
Response.text("created")
    .status(201)
    .header("Location", "/items/42")
```

| Mutator | Effect |
|---|---|
| `.status(code)` | override the status code (chainable) |
| `.header(name, value)` | append a header (chainable) |

JSON responses take a Buff `Map<String, _>` literal, which `buff-web` serializes
via `serde_json`:

```buff
Response.json({ "id": 42, "title": "write the book", "done": false })
```

The Map-literal syntax `{ "key": value, ... }` is the same one Buff uses for
`HashMap` construction (see [Chapter 6 §6.4](./chapter-6.md)). It serializes
cleanly to a JSON object.

## 3.5 A JSON todo API 🔶

Putting routes + JSON requests + JSON responses together. This is a complete
(in-memory) todo API:

```buff
from "buff/web" import Web, Request, Response

// In-memory store. A real app would use buff-db (Chapter 7 §7.6) here.
let mut todos: Map<Int, Map<String, Unknown>> = {}
let mut next_id: Int = 1

func main():
    app = Web.new()

    // GET /todos — list all.
    app.get(path: "/todos", handler: { _req =>
        Response.json({ "todos": todos.values() })
    })

    // POST /todos — create. Expects {"title": "..."}.
    app.post(path: "/todos", handler: { req =>
        match req.json():
            Ok(body):
                let title = body["title"].or(default: "untitled")
                let id = next_id
                next_id = next_id + 1
                todos[id] = { "id": id, "title": title, "done": false }
                Response.json(todos[id]).status(201)
            Err(_):
                Response.json({ "error": "invalid json" }).status(400)
    })

    // GET /todos/{id} — fetch one.
    app.get(path: "/todos/{id}", handler: { req =>
        let id = Int(req.path().split("/")[3]).or(default: 0)
        match todos[id]:
            Some(todo):
                Response.json(todo)
            None:
                Response.json({ "error": "not found" }).status(404)
    })

    // DELETE /todos/{id} — delete.
    app.delete(path: "/todos/{id}", handler: { req =>
        let id = Int(req.path().split("/")[3]).or(default: 0)
        match todos.delete(id):
            Some(_):
                Response.status_only(204)
            None:
                Response.json({ "error": "not found" }).status(404)
    })

    print("todo API listening on http://0.0.0.0:8080")
    app.listen(port: 8080)
```

Try it:

```bash
buff run todo_api.buff &
curl -X POST http://localhost:8080/todos -d '{"title":"write chapter 3"}'
# {"id":1,"title":"write chapter 3","done":false}
curl http://localhost:8080/todos
# {"todos":[{"id":1,"title":"write chapter 3","done":false}]}
curl -X DELETE http://localhost:8080/todos/1
# (204 No Content)
```

A few things to notice:

- **`{ _req => ... }`** — a lambda that ignores its argument. The leading `_`
  is the conventional name for "I have to take this parameter but I won't use
  it".
- **`body["title"].or(default: "untitled")`** — `Map` indexing returns
  `Option<V>`; `.or(default: ...)` unwraps it.
- **`Int(req.path().split("/")[3])`** — the `Int(x)` prelude function converts
  a `String` to an `Int`, returning `Option<Int>` (the conversion can fail).
  Combined with `.or(default: 0)` this is a safe parse.
- **`match todos.delete(id):`** — `Map.delete(key)` returns `Option<V>`:
  `Some(removed_value)` if the key existed, `None` otherwise. We use that to
  decide between 204 and 404.

## 3.6 Middleware

Middleware sits in front of your routes and can short-circuit or decorate. The
`app.middleware(fn)` method registers a function in the dispatch chain:

```buff
func main():
    app = Web.new()

    // Logging middleware — runs before every handler.
    app.middleware({ req =>
        print("{req.method()} {req.path()}")
        // Return None to delegate to the next handler.
        // Return Some(Response...) to short-circuit.
        None
    })

    // CORS middleware — adds headers to every response.
    app.middleware({ req =>
        // In a real app you'd mutate the response; the MVP's middleware
        // surface is request-side only for now.
        None
    })

    app.get(path: "/", handler: { _req => Response.text("ok") })
    app.listen(port: 8080)
```

For an N-middleware chain, the call stack grows by N frames per request — fine
for the typical N=2..5. Deep chains (N>20) would benefit from an iterative
rewrite, deferred to v1.18+.

## 3.7 JSON without a web server

The `Json` prelude type is independent of `buff-web`. You can parse and
serialize JSON anywhere:

```buff
func main():
    let text = "{ \"name\": \"Ada\", \"age\": 36 }"
    match Json.parse(text):
        Ok(map):
            let name = map["name"].or(default: "?")
            print("hello, {name}")
        Err(e):
            print("parse failed: {e}")

    let data = { "greeting": "hello", "count": 7 }
    let serialized = Json.stringify(data)
    print(serialized)
```

`Json.parse` returns `Result<Map<String, Unknown>, Error>`; `Json.stringify`
returns `String`. They round-trip cleanly. The same shape applies to `Toml`,
`Yaml`, and `Csv` — each is a namespace prelude type with `.parse()` and
`.stringify()` associated functions. See [Chapter 7 §7.2](./chapter-7.md).

## 3.8 Building for production

The default `buff build` (no flags) produces an unoptimized debug binary.
For a production server:

```bash
buff build --release todo_api.buff
```

`--release` activates `opt-level=3` + LTO across crates. The result is a native
binary that competes with hand-tuned Go or Java servers on throughput, without
a garbage collector and without a runtime pause.

For a server, **do not** use `--minimal` ([Chapter 2 §2.8](./chapter-2.md)).
`--minimal` sets `panic=abort`, which is fine for a CLI that exits on error but
wrong for a long-running server that should keep serving after a single bad
request triggers a panic. The `buff-web` wrapper wraps every handler body in
`catch_unwind` (per the FFI safety guide R6), so a panicking handler becomes a
`500 Internal Server Error` rather than a process abort — but only under the
default unwinding panic strategy that `--release` preserves.

## 3.9 What's intentionally out of scope

The `buff-web` MVP is deliberately small. These are explicitly **not** in
scope:

| Feature | Where to get it |
|---|---|
| **WebSocket** | The stdlib `WebSocket.connect(url)` prelude type (Chapter 7). |
| **Template rendering** | The `buff-template` crate (T19). |
| **ORM / database** | The `buff-db` crate (T18) — Chapter 7 §7.6. |
| **Routing via macros** | Runtime registration only — no proc macros across the FFI boundary. |
| **Path-param extraction** | `req.path()` only for the MVP; `req.param("id")` deferred to v1.18+. |
| **HEAD / OPTIONS / TRACE / CONNECT** | The five canonical verbs only. |
| **GraphQL / gRPC / WebDAV / HTTP/2 push** | Explicitly deferred. |

This is the same "do the 80% well, defer the 20%" philosophy that shapes every
Buff framework crate. The MVP exists today and is tested; the long tail ships
in later waves.

## 3.10 Recap

- **`buff-web`** wraps axum 0.8 + tokio + serde_json behind a safe Buff API.
- `Web.new()` → `app.get/post/put/delete/patch(path:, handler:)` →
  `app.listen(port:)`.
- Handlers are *synchronous* closures `{ req => Response... }`. Buff has no
  `await` — async lives inside the wrapper.
- `Request` accessors: `.method()`, `.path()`, `.header(name)`, `.body()`,
  `.json()`.
- `Response` builders: `.text(s)`, `.json(value)`, `.status_only(code)`,
  chainable `.status(code)` and `.header(name, value)`.
- `app.middleware(fn)` registers request-side middleware.
- Build production servers with `--release`, **not** `--minimal`.

---

*Next: [Chapter 4 — GPU Compute](./chapter-4.md)*
