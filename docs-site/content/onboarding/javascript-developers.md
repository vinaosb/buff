+++
title = "Buff for JavaScript developers"
weight = 49
+++

# Buff for JavaScript developers

> JavaScript is the lingua franca of the web, but its dynamic typing,
> `Promise`-coloring problem, and `undefined`-sightings make large
> codebases fragile. Buff keeps the ergonomics you love (closures,
> async/await vibe, JSON-native data) and adds real static types,
> native performance, transparent parallelism, and a borrow-checker-
> free ownership model. If you've written TypeScript, you already
> know most of Buff's surface; if you've written React, the
> `@State`/`@Published` property wrappers will feel familiar.

This guide assumes you can read modern JavaScript (ES2020+) and have
seen TypeScript, `async`/`await`, Promises, and React hooks. You'll
be productive in Buff in 30 minutes.

## Why Buff?

For JavaScript and TypeScript developers, the value prop is:

1. **Real static types without TS's tooling tax.** TypeScript's types
   are erased at runtime; Buff's types are enforced by the compiler
   and erased to native machine representation. No `tsc` step, no
   `tsconfig.json`, no `any` escape hatch that silently breaks.
2. **Native performance, no Node startup tax.** Buff compiles to a
   native binary via LLVM. No V8 startup, no JIT warmup, no GC
   pauses. Cold-start is microseconds, not milliseconds.
3. **No `Promise` coloring.** JavaScript's async/await still colors
   functions — an `async` function returns a Promise, and forgetting
   `await` is a classic bug. Buff has no `await` keyword; async
   propagates automatically.
4. **No `null` or `undefined`.** Absence is `Option<T>`; failure is
   `Result<T, E>`. Both are matched exhaustively. No more
   `if (x === null || x === undefined)` checks.
5. **Real parallelism, not just concurrency.** JavaScript is
   single-threaded (web workers and worker_threads are clunky). Buff
   runs across all CPU cores via Rayon, and the same loop can be
   dispatched to GPU via `@prefer(gpu)`.
6. **Same closure + combinator ergonomics.** `[1,2,3].map(x => x*2)`
   in JS is `[1, 2, 3].map({ x => x * 2 })` in Buff. The shape is
   nearly identical.
7. **Familiar web framework patterns.** `buff-web` (v1.15+) mirrors
   Express/Koa/Fastify for HTTP servers; `.buffhtml` SFC files mirror
   Vue/Svelte for reactive UIs.

The trade-off: Buff doesn't run in the browser (yet — wasm is on the
roadmap), the npm ecosystem is larger, and the JS event loop is more
mature for I/O-heavy workloads. For CLI tools, servers, build scripts,
and compute-heavy work, Buff is competitive today.

## Syntax mapping table

### Fundamentals

| JavaScript / TypeScript | Buff | Notes |
|---|---|---|
| `function f() {` | `func f():` | Colon + indent instead of braces. |
| `const f = () => {` | `func f():` | Same Buff syntax for named/anonymous. |
| `const x = 5` | `let x = 5` | `let` for both const and let in JS. |
| `let x = 5` | `let mut x = 5` | `mut` opts into mutation. |
| `var x = 5` | (no equivalent — use `let`/`let mut`) | No hoisting. |
| `// comment` | `// comment` | Same. |
| `/* block */` | (none — use multiple `//`) | No block comments. |
| `console.log(x)` | `print(x)` | In prelude. |
| 2-space indent (JS convention) | 4-space indent (Buff rule) | Tabs forbidden. |
| `;` (semicolons) | (none — Buff has no semicolons) | Statements end at newline. |

### Types (TypeScript)

| TypeScript | Buff | Notes |
|---|---|---|
| `let x: number = 5` | `let x: Int = 5` (or `let x = 5`) | `number` → `Int` or `Float`. |
| `let x: string = "hi"` | `let x: String = "hi"` | Capital S. |
| `let x: boolean = true` | `let x: Bool = true` | Capital B. |
| `let x: number[] = [1, 2]` | `let x: Vector<Int> = [1, 2]` | Generic syntax. |
| `let x: Array<number>` | `let x: Vector<Int>` | Same. |
| `let x: { a: number }` | `struct X: a: Int` then `let x: X` | Object type → struct. |
| `let x: [number, string]` | (no equivalent — use a struct) | No tuples. |
| `type X = { ... }` | `struct X: ...` | Renamed. |
| `interface X { ... }` | `trait X: ...` | Renamed. |
| `let x: any` | (forbidden) | Use generics. |
| `let x: unknown` | (no equivalent) | Match on type or use trait. |
| `let x: null` | `Option<T>` with `None` | No `null`. |
| `let x: undefined` | `Option<T>` with `None` | No `undefined`. |
| `let x: string \| null` | `Option<String>` | Same idea. |
| `type Result<T> = { ok: true, value: T } \| { ok: false, error: Error }` | `Result<T, Error>` | Built-in. |
| `Promise<T>` | `T` (async func returns `T`) | No Promise — async is transparent. |
| `function f<T>(x: T): T` | `func f<T>(x: T) -> T:` | Same generics syntax. |
| `class Foo extends Bar` | (no inheritance) | Use trait + composition. |
| `enum Color { Red }` | `enum Color: Red` | Same idea. |
| `as const` | `const X = ...` | Module-level constant. |
| `as Foo` (cast) | (no equivalent — use match) | No type assertions. |

### Strings and template literals

| JavaScript | Buff | Notes |
|---|---|---|
| `"hi"` / `'hi'` | `"hi"` | Double quotes only. |
| `` `hi ${name}` `` | (no equivalent — use `+`) | No template literals yet. |
| `String(x)` | `x.string()` | Method-style. |
| `x.toString()` | `x.string()` | Renamed. |
| `s.length` | `s.len()` | Method. |
| `s.toUpperCase()` | `s.to_uppercase()` | Renamed. |
| `s.split(",")` | `s.split(",")` | Same. |
| `s.includes("x")` | `s.contains("x")` | Renamed. |
| `s.trim()` | `s.trim()` | Same. |
| `s.replace("a", "b")` | `s.replace("a", "b")` | Same. |
| `s + "!"` | `s + "!"` | Same. |
| `parseInt(s)` | `s.parse_int()` | Method; returns `Result`. |
| `parseFloat(s)` | `s.parse_float()` | Method; returns `Result`. |

### Collections

| JavaScript | Buff | Notes |
|---|---|---|
| `[1, 2, 3]` | `[1, 2, 3]` | Array → `Vector<T>`. |
| `arr.push(x)` | `v.push(x)` | Same. |
| `arr.pop()` | `v.pop()` | Same; returns `Option<T>`. |
| `arr.length` | `v.len()` | Method. |
| `arr[i]` | `v[i]` | Same. |
| `arr.map(x => x * 2)` | `v.map({ x => x * 2 })` | Closure syntax differs. |
| `arr.filter(x => x > 0)` | `v.filter({ x => x > 0 })` | Same. |
| `arr.reduce((a, b) => a + b, 0)` | `v.reduce({ a, b => a + b }, 0)` | Same. |
| `arr.find(x => x.id === 1)` | `v.find({ x => x.id == 1 })` | Returns `Option<T>`. |
| `arr.includes(x)` | `v.contains(x)` | Renamed. |
| `arr.slice(1, 3)` | `v.slice(1, 3)` | Same. |
| `arr.concat(other)` | `v.concat(other)` | Same. |
| `arr.join(",")` | `v.join(",")` | Same. |
| `[...arr1, ...arr2]` | `arr1.concat(arr2)` | No spread. |
| `{ a: 1, b: 2 }` | `{ "a": 1, "b": 2 }` | Object → Map. |
| `obj.a` | `map.get("a")` | Method; returns `Option<T>`. |
| `obj["a"]` | `map.get("a")` | Same. |
| `Object.keys(obj)` | `map.keys()` | Method. |
| `Object.values(obj)` | `map.values()` | Method. |
| `Object.entries(obj)` | `map.entries()` | Method. |
| `delete obj.a` | `map.remove("a")` | Method. |
| `new Map()` | `Map.new()` | Or literal `{}`. |
| `new Set()` | `Set.new()` | Same. |

### Control flow

| JavaScript | Buff | Notes |
|---|---|---|
| `if (c) { }` | `if c:` | Colon + indent. |
| `else if` | `else if` | Same. |
| `switch (x) { case A: break; }` | `match x { Pat => body, }` | Renamed; no `break` needed. |
| `for (let i = 0; i < 10; i++) { }` | `for i in 0..10:` | C-style for is gone. |
| `for (const x of arr) { }` | `for x in arr:` | Same shape. |
| `for (const k in obj) { }` | `for k in map.keys():` | Renamed. |
| `while (c) { }` | (no equivalent) | Use `for x in iter` or recursion. |
| `do { } while (c);` | (no equivalent) | Use recursion. |
| `break`, `continue` | `break`, `continue` | Same. |
| `try { } catch (e) { }` | `match f() { Ok(v) => ..., Err(e) => ... }` | No try/catch; use Result. |
| `throw new Error("x")` | `return Error("x")` | Returns a Result. |
| `finally { cleanup() }` | (no equivalent — RAII handles cleanup) | Drop-on-scope-exit. |

### Functions and lambdas

| JavaScript | Buff | Notes |
|---|---|---|
| `function f(x) { return x; }` | `func f(x): return x` | Same shape. |
| `const f = (x) => x * 2` | `func f(x): return x * 2` | Or `let f = { x => x * 2 }` (closure). |
| `const f = x => x * 2` | `let f = { x => x * 2 }` | Closure syntax. |
| `(x, y) => x + y` | `{ x, y => x + y }` | Multi-param closure. |
| `function f(x = 5) { }` | `func f(x: Int = 5):` | Default args supported. |
| `function f(...xs) { }` | `func f(xs: Vector<Int>):` | Rest params → vector. |
| `f.apply(null, args)` | `f(...args)` (spread planned) | Or pass a vector. |
| `f.bind(this)` | (no equivalent — closures capture by move) | No `this`. |
| `this` | (no equivalent) | No `this`, no prototypes. |
| `class Foo { constructor() { } }` | `struct Foo:` + `func Foo.new():` | Constructor via `.new()`. |
| `class Foo extends Bar { }` | (no inheritance) | Use trait + composition. |
| `new Foo()` | `Foo.new()` | No `new` keyword. |

### Async

| JavaScript / TypeScript | Buff | Notes |
|---|---|---|
| `async function f() { }` | `async func f():` | Same keyword. |
| `await f()` | `f()` | **No `await`.** Async propagates. |
| `Promise<T>` | `T` (return type) | Async funcs return `T` directly. |
| `Promise.resolve(x)` | (just `return x`) | No Promise wrapper. |
| `Promise.reject(e)` | `return Error(e)` | Returns a Result. |
| `Promise.all([a, b])` | `spawn` + `task.result()` × N | Manual gather. |
| `Promise.race([a, b])` | (planned via `select`) | Or `Channel.recv_timeout(d)`. |
| `Promise.allSettled(...)` | (no equivalent) | Use Vector of Results. |
| `async () => { }` | `async func():` or `spawn { }` | Async closure. |
| `setTimeout(fn, ms)` | `spawn { sleep(ms); fn() }` | Or `sleep(Duration.millis(ms))` in async. |
| `setInterval(fn, ms)` | (planned) | Or loop + sleep. |
| `fetch(url)` | `HttpClient.get(url).send()` | Wraps `reqwest`. |
| `EventEmitter` | `buff-pubsub` `EventBus` (T41) | In-process pub/sub. |

### Object-oriented features

| JavaScript | Buff | Notes |
|---|---|---|
| `class Foo { method() { } }` | `struct Foo:` + `impl Foo: func method():` | Two-part definition. |
| `class Foo extends Bar { }` | (no inheritance) | Use trait + composition. |
| `class Foo implements IBaz { }` | `impl IBaz for Foo:` | Same idea, explicit. |
| `constructor() { }` | `func Foo.new(...):` | Factory function convention. |
| `get prop()` / `set prop()` | (use methods) | No getters/setters. |
| `static method()` | `func Foo.method():` | Methods on the type. |
| `#privateField` | (no equivalent — everything is module-private by default) | Use module structure. |
| `Symbol.iterator` | `trait Iterable<T>` | Same idea. |
| `Proxy` / `Reflect` | (no equivalent) | Static types only. |
| Decorators `@decorator` | `@attribute` (`@test`, `@State`, etc.) | Compiler hints, not runtime wrappers. |

## Tooling migration

The JavaScript toolchain is famously complex (npm/yarn/pnpm/bun +
webpack/vite/rollup/esbuild + babel/swc + eslint/prettier + jest/vitest
+ Cypress/Playwright). Buff collapses all of this into one binary.

| JavaScript | Buff | Notes |
|---|---|---|
| `node file.js` | `buff run <file>` | Compile + run. |
| `npm install` | `buff deps` | Resolve + download deps. |
| `npm install pkg` | `buff add pkg` | Add to `buff.toml`. |
| `npm install -g pkg` | `buff install pkg` | Install a binary. |
| `npm update` | `buff update` | Update a dep. |
| `npm outdated` | `buff outdated` | List outdated deps. |
| `npm publish` | `buff publish` | Publish to `buff-registry`. |
| `npm run build` | `buff build` | Compile to a native binary. |
| `npm run dev` | `buff run <file>` | Compile + execute. |
| `npm test` | `buff test` | Run `@test` functions. |
| `npm start` | `buff run src/main.buff` | Run the entry point. |
| `package.json` | `buff.toml` | Manifest. |
| `package-lock.json` | `buff.lock` | Lockfile (gitignored). |
| `node_modules/` | `~/.buff/cache/` | Global cache, not per-project. |
| `nvm` / `fnm` / `volta` | `buffup` | Version manager. |
| `tsc` (TypeScript compiler) | `buff check` | Statically type-checks. |
| `babel` / `swc` | (none — Buff is the compiler) | One tool, no transpile chain. |
| `webpack` / `vite` / `esbuild` | `buff build` | One bundler. |
| `eslint` / `biome` / `oxlint` | `buff check` (lints included) | Linter is built-in. |
| `prettier` | `buff fmt` | Indent-based formatter. |
| `jest` / `vitest` / `mocha` | `buff test` (built-in) | Test runner is built-in. |
| `playwright` / `puppeteer` | (planned) | Browser automation. |
| `webpack-dev-server` | `buff ui dev` (T131) | WebSocket hot-reload server. |
| `storybook` | (planned) | Component playground. |
| `npm scripts` | (just write shell or use `buff.toml` `[scripts]`) | Built-in. |

### Build vs run

In JavaScript, you `node file.js` and V8 interprets it (with JIT).
In Buff, there's a compile step:

```bash
buff run src/main.buff          # compile + run, throw away binary
buff build src/main.buff        # compile, keep binary
buff check src/main.buff        # type-check only (fast, like tsc --noEmit)
buff fmt                        # format (like prettier --write)
buff test                       # run @test functions (like jest)
```

`buff check` is what your editor runs on save (like tsserver does in
the background). `buff run` is for development. `buff build` is for
distribution. The output is a native binary — you ship one file.

### The project layout

A `buff new my_app` looks like:

```
my_app/
├── buff.toml          # project manifest (like package.json)
├── src/
│   └── main.buff      # entry point (like src/index.js)
└── tests/
    └── test_main.buff # test file (like src/main.test.js)
```

Compare to a typical npm project:

```
my_app/
├── package.json
├── src/
│   └── index.js
└── __tests__/
    └── main.test.js
```

The shape is the same. The differences:

- No `node_modules/` (Buff caches globally in `~/.buff/cache/`).
- No `dist/` or `build/` (Buff writes to `target/`).
- No `tsconfig.json` (types are configured in `buff.toml`).
- No `.eslintrc` / `.prettierrc` (linting is built-in and
  opinionated).

### Dependency declaration

In `package.json`:

```json
{
  "name": "my_app",
  "version": "1.0.0",
  "dependencies": {
    "express": "^4.18",
    "axios": "^1.6"
  },
  "devDependencies": {
    "jest": "^29.7",
    "typescript": "^5.3"
  }
}
```

In `buff.toml`:

```toml
[package]
name = "my_app"
version = "1.0.0"
edition = "2021"

[deps]
buff_web = "1.0"
buff_http_client = "1.0"
```

Note: there's no `devDependencies` split in Buff — `buff test`
discovers `@test`-marked functions anywhere in the project. Test
framework is built-in.

## Ecosystem mapping

The npm ecosystem is the largest in the world. Buff doesn't try to
clone it; common JS libraries map onto Buff prelude types, `buff-*`
framework crates, or Rust crates reachable via `extern`.

| JavaScript library | Buff equivalent | Notes |
|---|---|---|
| `axios` / `node-fetch` / `got` | `HttpClient` (prelude) | Wraps `reqwest`. |
| `express` / `koa` / `fastify` | `buff-web` (v1.15+) | Web framework. |
| `next` / `nuxt` / `remix` | (planned — `buff-ui-dioxus` for SSR) | Full-stack framework. |
| `react` / `vue` / `svelte` | `buff-ui-dioxus` + `.buffhtml` | RSX-based UI. |
| `lodash` | (mostly built-in) | `Vector<T>` has `.map`, `.filter`, etc. |
| `underscore` | (mostly built-in) | Same. |
| `ramda` | (mostly built-in) | Closures + combinators. |
| `moment` / `date-fns` / `dayjs` | `DateTime` (prelude) | Date arithmetic. |
| `uuid` | `UUID` (prelude) | UUIDv4. |
| `nanoid` | `Random.string(N)` | Short random IDs. |
| `bcrypt` / `argon2` | `Hash` / `HMAC` (prelude) | Hashing (no bcrypt wrapper yet). |
| `jsonwebtoken` | (planned — `buff-auth` v1.16+) | JWT. |
| `cors` | (built into `buff-web`) | CORS middleware. |
| `body-parser` | (built into `buff-web`) | Body parsing. |
| `multer` | (planned) | File uploads. |
| `ws` / `socket.io` | `WebSocket` (prelude) | Async WebSocket client. |
| `mongoose` / `prisma` / `sequelize` | `buff-db` (v1.15+) | Database ORM. |
| `redis` / `ioredis` | `buff-cache` (v1.16+, in-memory MVP) | Distributed cache. |
| `pg` / `mysql2` | `buff-db` (v1.15+) | Postgres/MySQL drivers. |
| `mongodb` | (planned) | Mongo driver. |
| `jest` / `vitest` | `buff test` (built-in) | Test runner. |
| `chai` / `should` | (built-in assertions) | `assert_eq`, `assert_true`. |
| `sinon` / `nock` | `buff-mock` (v1.13+) | Mocking framework. |
| `faker` | `buff-fake` (v1.17+) | Fake data generation. |
| `playwright` / `puppeteer` | (planned) | Browser automation. |
| `cheerio` | `buff-scrape` (v1.17+) | HTML scraping. |
| `zod` / `joi` / `yup` | `buff-validate` (v1.16+) | Runtime validation. |
| `pino` / `winston` / `bunyan` | `Log` (prelude) | Structured logging. |
| `chalk` / `picocolors` | (planned) | Terminal colors. |
| `commander` / `yargs` / `meow` | `Args` (prelude) | CLI parsing built-in. |
| `inquirer` / `prompts` | (planned) | Interactive prompts. |
| `dotenv` | `Env` (prelude) | Env vars are read directly. |
| `glob` / `chokidar` | `Filesystem` / `buff watch` (T64) | File watching. |
| `fs-extra` | `Filesystem` (prelude) | File I/O. |
| `sharp` / `jimp` | `buff-image` (v1.14+) | Image processing. |
| `exceljs` / `xlsx` | (planned) | Spreadsheet I/O. |
| `pdfkit` / `puppeteer` (PDF) | (planned) | PDF generation. |
| `nodemailer` | `buff-email` (v1.17+) | Email sending. |
| `agenda` / `bull` / `bullmq` | `buff-jobs` (v1.16+) | Background jobs. |
| `amqp` / `kafkajs` | `buff-pubsub` (T41, in-process MVP) | Pub/sub. |

For npm packages without a Buff equivalent, you can almost always
find a Rust crate that does the same thing and bind it via `extern`
(see the [Rust developer guide](./rust-developers/) for the FFI
story).

## Hello World, side by side

A canonical first program. Print a greeting, count to three, do a
tiny calculation.

### JavaScript

```javascript
function greet(name) {
    return `Hello, ${name}!`;
}

function main() {
    for (let i = 1; i <= 3; i++) {
        console.log(`count: ${i}`);
    }
    const args = process.argv.slice(2);
    const who = args.length > 0 ? args[0] : "World";
    console.log(greet(who));
    console.log(`2 + 2 = ${2 + 2}`);
}

main();
```

### TypeScript

```typescript
function greet(name: string): string {
    return `Hello, ${name}!`;
}

function main(): void {
    for (let i = 1; i <= 3; i++) {
        console.log(`count: ${i}`);
    }
    const args = process.argv.slice(2);
    const who = args.length > 0 ? args[0] : "World";
    console.log(greet(who));
    console.log(`2 + 2 = ${2 + 2}`);
}

main();
```

### Buff

```buff
func greet(name: String) -> String:
    return "Hello, " + name + "!"

func main():
    for i in 1..=3:
        print("count: " + i.string())
    let args = Args.all()
    let who = if args.len() > 1 { args[1] } else { "World" }
    print(greet(who))
    print("2 + 2 = " + (2 + 2).string())
```

### Line-by-line

- **`function greet(name) { ... }`** → `func greet(name: String) ->
  String:`. `function` → `func`; param syntax `name: Type`; return
  `-> Type`; braces → colon + indent.
- **`` `Hello, ${name}!` ``** → `"Hello, " + name + "!"`. Buff has
  no template literals yet — use string concatenation. (Interpolation
  is planned.)
- **`for (let i = 1; i <= 3; i++)`** → `for i in 1..=3`. C-style for
  is gone; use range syntax. `1..=3` is inclusive.
- **`` `count: ${i}` ``** → `"count: " + i.string()`. Numbers don't
  auto-stringify; call `.string()` explicitly.
- **`console.log(...)`** → `print(...)`. In prelude; adds newline.
- **`process.argv.slice(2)`** → `Args.all()`. The `Args` module
  wraps OS args; note Buff doesn't slice off the first two (you index
  from `[0]`).
- **`args.length > 0 ? args[0] : "World"`** → `if args.len() > 1 {
  args[1] } else { "World" }`. No ternary; `if` is an expression.
- **`main()`** at the bottom → gone. `func main():` is the entry
  point; the compiler calls it for you.

## Async model: Buff async-transparent vs JS Promise/await

This is the biggest JS→Buff mental shift. In JavaScript, `async`/`await`
is syntactic sugar over Promises — every `async` function returns a
Promise, and you must `await` it at the call site. Forgetting `await`
is a classic bug:

```javascript
// JavaScript
async function fetchUser(uid) {
    const resp = await fetch(`/users/${uid}`);
    return await resp.json();
}

async function main() {
    const a = fetchUser(1);        // BUG: forgot await
    const b = await fetchUser(2);
    console.log(a, b);             // a is a Promise, not a User
}

main();
```

In Buff, there's no `await` keyword. You call `fetchUser(1)` like any
other function — the compiler inserts the equivalent of `.await`
automatically:

```buff
async func fetch_user(uid: Int) -> User:
    let resp = HttpClient.get("/users/" + uid.string()).send()?
    return resp.json()?

func main():
    let a = fetch_user(1)          // no await needed
    let b = fetch_user(2)
    print(a, b)
```

Notice:

1. **No `await` keyword anywhere.** `fetch_user(1)` returns a `User`
   directly, not a Promise.
2. **`main` is not declared `async`.** The compiler sees that `main`
   calls an async function and propagates async-ness upward
   automatically, then emits the equivalent of `async function main()`
   on the generated Rust.
3. **No "function coloring" bug.** You can't forget to `await` — the
   concept doesn't exist.

This eliminates an entire class of JS bugs. The trade-off is that
Buff's async is less flexible than JS's — you can't easily build a
`Promise.all` of mixed-type tasks, for example. You use `spawn` +
`task.result()` instead.

## Callback patterns → closures

JavaScript makes heavy use of callbacks (event handlers, Array methods,
async operations). Buff's closures are conceptually identical, with a
different syntax.

### JavaScript

```javascript
// Array methods
const doubled = [1, 2, 3].map(x => x * 2);
const evens = [1, 2, 3, 4].filter(x => x % 2 === 0);
const sum = [1, 2, 3].reduce((acc, x) => acc + x, 0);

// Event handler
button.addEventListener("click", event => {
    console.log("clicked", event.target);
});

// Higher-order function
function withTimer(fn) {
    const start = Date.now();
    const result = fn();
    console.log(`took ${Date.now() - start}ms`);
    return result;
}
```

### Buff

```buff
// Vector methods
let doubled = [1, 2, 3].map({ x => x * 2 })
let evens = [1, 2, 3, 4].filter({ x => x % 2 == 0 })
let sum = [1, 2, 3].reduce({ acc, x => acc + x }, 0)

// Higher-order function
func with_timer<T>(fn: func() -> T) -> T:
    let start = DateTime.now()
    let result = fn()
    let elapsed = DateTime.now() - start
    print("took " + elapsed.string())
    return result
```

Differences:

- **Closure syntax**: JS uses `(params) => body` or `params => body`;
  Buff uses `{ params => body }`. Braces required.
- **Multi-param**: JS `(a, b) => ...`; Buff `{ a, b => ... }`.
- **No `this`**: Buff closures capture by move; there's no `this`
  binding to worry about.
- **Event handlers**: Buff doesn't have a built-in event emitter type
  in the prelude yet — use `buff-pubsub` `EventBus` (T41) for
  in-process pub/sub, or `Channel<T>` for producer/consumer.

## Web frameworks: buff-web vs Express/Koa

Buff's web framework (`buff-web`, v1.15+) mirrors Express/Koa/Fastify
in shape — define routes, attach handlers, return responses.

### Express (JavaScript)

```javascript
const express = require("express");
const app = express();

app.get("/", (req, res) => {
    res.json({ hello: "world" });
});

app.get("/users/:id", async (req, res) => {
    const user = await fetchUser(req.params.id);
    res.json(user);
});

app.listen(3000);
```

### buff-web (Buff)

```buff
import buff_web

func main():
    let app = buff_web.App.new()
    app.get("/", { req, res => res.json({ "hello": "world" }) })
    app.get("/users/{id}", { req, res =>
        let id = req.param("id").parse_int()?
        let user = fetch_user(id)?
        res.json(user)
    })
    app.listen(3000)

func fetch_user(id: Int) -> Result<Map<String, String>, Error>:
    let db = Database.connect("...")?
    let row = db.query_one("SELECT * FROM users WHERE id = $1", [id])?
    return Ok(row)
```

The shape is nearly identical:

- `app.get(path, handler)` instead of `app.get(path, (req, res) => ...)`.
- Handlers are closures taking `(req, res)`.
- Path params use `{id}` instead of `:id`.
- Async is transparent — no `await`.

Buff's framework is younger than Express, so expect fewer middleware
options and a smaller ecosystem. The core routing + JSON + middleware
pipeline is in place.

## npm → buff add

Buff's package model is closer to npm than to cargo's workspace
model — one project, one manifest, declarative deps.

```bash
# npm
npm init                    # creates package.json
npm install express         # adds "express" to dependencies
npm install                 # downloads everything in package.json
npm update express          # bumps a single dep
npm outdated                # lists outdated deps
npm publish                 # uploads to npmjs.com

# Buff
buff init                   # creates buff.toml
buff add buff_web           # adds buff_web = "1.0" to buff.toml
buff deps                   # resolves and downloads everything
buff update buff_web        # bumps a single dep
buff outdated               # lists outdated deps
buff publish                # uploads to buff-registry
```

The `buff-registry` server (T126-T127) is the equivalent of npmjs.com.
It's pure-Rust (axum + semver, in-memory storage today; persistent
storage planned). You can self-host a private registry or use the
community instance.

For npm packages without a Buff wrapper, declare the underlying Rust
crate in `[rust-deps]` and bind via `extern` (see the [Rust developer
guide](./rust-developers/) for FFI details).

## TypeScript types → Buff type inference

TypeScript's type system is structurally typed and runs at... well,
only at compile time (via `tsc`). Buff's types are nominally typed
and enforced by the compiler once, at compile time. The biggest
differences:

| TypeScript | Buff | Notes |
|---|---|---|
| `interface Foo { x: number }` | `trait Foo: func x() -> Int` | Buff traits have methods, not fields. |
| `type Foo = { x: number }` | `struct Foo: x: Int` | Object type → struct. |
| `class Foo { x: number }` | `struct Foo: x: Int` | Same. |
| `Foo & Bar` (intersection) | (no equivalent) | Compose via trait bounds. |
| `Foo \| Bar` (union) | (no equivalent) | Use an enum. |
| `Partial<Foo>` | (no equivalent) | Use `Option<Type>`. |
| `Readonly<Foo>` | (default — everything is immutable) | `mut` opts in. |
| `Record<K, V>` | `Map<K, V>` | Built-in. |
| `Pick<Foo, "a" \| "b">` | (no equivalent) | Define a new struct. |
| `Omit<Foo, "a">` | (no equivalent) | Define a new struct. |
| `ReturnType<typeof f>` | (inferred) | The compiler knows. |
| `Parameters<typeof f>` | (inferred) | The compiler knows. |
| `as Foo` (cast) | (no equivalent) | No type assertions. |
| `satisfies Foo` | (no equivalent) | Just annotate `: Foo`. |
| `!` (non-null assertion) | (no equivalent — match the Option) | No null assertions. |
| `?.` (optional chaining) | `?.` (desugars to `and_then`) | Similar idea. |
| `??` (nullish coalescing) | `??` (desugars to `BinaryOp`) | Same. |

Buff's inference is aggressive — you rarely write types except at
function boundaries:

```buff
func main():
    let nums = [1, 2, 3]                       // Vector<Int>
    let doubled = nums.map({ x => x * 2 })      // Vector<Int>
    let total = doubled.reduce({ a, b => a + b }, 0)  // Int
    print(total)                                // 12
```

The compiler infers `Vector<Int>` from the literals, `Int` for the
closure params from the `.map()` signature, and `Int` for the
accumulator. You'd write annotations only on public APIs.

## React hooks → `@State` property wrappers

If you've used React hooks (`useState`, `useEffect`, `useMemo`,
`useReducer`), Buff's property wrappers (T56, Swift-inspired) will
feel familiar. They're the reactive primitives for `.buffhtml` SFC
components.

### React (JavaScript)

```jsx
import { useState, useEffect } from "react";

function Counter() {
    const [count, setCount] = useState(0);

    useEffect(() => {
        console.log(`count is ${count}`);
    }, [count]);

    return (
        <div>
            <p>{count}</p>
            <button onClick={() => setCount(count + 1)}>+1</button>
        </div>
    );
}
```

### Buff (`.buffhtml` SFC)

```buffhtml
---
<script>
buff:
    @State count = 0

    func increment():
        count.set(count.get() + 1)
</script>

<template>
    <div>
        <p>{count.get()}</p>
        <button onClick="increment">+1</button>
    </div>
</template>
---
```

Differences:

- **`@State`** declares a reactive cell. The cell has `.get()`,
  `.set(v)`, and `.update(fn)` methods.
- **No `useEffect`** — lifecycle hooks are component methods
  (`on_mount`, `on_update`, `on_unmount`); see the buff-ui component
  lifecycle docs.
- **`@Published`** is the equivalent of React Context — broadcasts
  changes to subscribers.
- **`@Cached`** is the equivalent of `useMemo` — recomputes only when
  dependencies change.
- **`@Observed`** is the equivalent of `useRef` — holds a mutable
  reference without triggering re-renders.

See
[`examples/property_wrappers_state.buff`](https://github.com/buff-lang/buff/blob/master/examples/property_wrappers_state.buff)
and
[`examples/counter.buffhtml`](https://github.com/buff-lang/buff/blob/master/examples/counter.buffhtml)
for runnable examples.

## Promise.all → spawn + gather

JavaScript's `Promise.all` runs an array of Promises concurrently and
waits for all of them. Buff's equivalent is `spawn` + `task.result()`:

### JavaScript

```javascript
async function main() {
    const [a, b, c] = await Promise.all([
        fetchUser(1),
        fetchUser(2),
        fetchUser(3),
    ]);
    console.log(a, b, c);
}
```

### Buff

```buff
func main():
    let task_a = spawn fetch_user(1)
    let task_b = spawn fetch_user(2)
    let task_c = spawn fetch_user(3)
    let a = task_a.result()
    let b = task_b.result()
    let c = task_c.result()
    print(a, b, c)
```

The shape is more verbose (you write each `task.result()` explicitly),
but it's also more explicit — you can see exactly which tasks are
running and when they're awaited.

For dynamic-length gather (the equivalent of `Promise.all(array)`),
use a `Channel<T>`:

```buff
func fetch_all(uids: Vector<Int>) -> Vector<User>:
    let (sender, receiver) = Channel.new(uids.len())
    for uid in uids:
        spawn fetch_user_into(uid, sender)
    var users = Vector<User>.new()
    for let user = receiver.recv():
        users.push(user)
    return users
```

See the [Async cookbook](../cookbook/async/) for the full pattern
catalog (timeout, select, gather, channels).

## Common pitfalls

The five things that trip up JavaScript developers most:

### 1. Forgetting `let` and `mut`

In JavaScript, `x = 5` introduces a new variable (in non-strict mode)
or throws (in strict mode). In Buff, you must write `let x = 5` for
the first assignment and `x = 5` for re-assignment. Re-assigning an
immutable binding is a compile error.

```buff
let x = 5           // first assignment — `let` required
x = 10               // ERROR: cannot assign to immutable binding
let mut y = 5        // mutable binding
y = 10                // OK
```

This matches TypeScript's `const` vs `let`, but flipped — Buff's
default is immutable (like `const`), and `mut` opts into mutation
(like `let`).

### 2. Numbers don't auto-stringify

In JavaScript, `` `x = ${n}` `` works for any `n`. In Buff, you must
call `.string()` explicitly:

```buff
let n = 42
print("x = " + n)              // ERROR: cannot concatenate String and Int
print("x = " + n.string())     // OK
print("x =", n)                // OK — print accepts multiple args
```

There are no template literals yet. Use `+` concatenation or
multi-arg `print`.

### 3. No `try`/`catch`

In JavaScript, you wrap fallible code in `try`/`catch`. In Buff,
every fallible function returns a `Result<T, E>` and you handle it
explicitly:

```javascript
// JavaScript
try {
    const value = JSON.parse(s);
} catch (e) {
    const value = null;
}
```

```buff
// Buff
let value = match Toml.parse(s) {
    Ok(v) => v,
    Err(_) => None,
}
// Or with ?? :
let value = Toml.parse(s) ?? None
```

There's no global exception handler. Errors are values. This matches
Rust and Go; the pitfall is assuming you can `try` your way out of a
bad state.

### 4. No `null` or `undefined`

In JavaScript, `null` and `undefined` are everywhere. In Buff,
absence is `Option<T>`:

```javascript
// JavaScript
let x = null;            // null
let y;                   // undefined
let z = foo();           // might return null
if (z !== null && z !== undefined) {
    console.log(z);
}
```

```buff
// Buff
let x: Option<Int> = None
let z = foo()             // returns Option<T>
match z {
    Some(v) => print(v),
    None => print("was none"),
}
```

The `?.` operator exists but means "and_then on Option", not
"null-safe access":

```buff
let nested = maybe.get("key")?.nested_field
// desugars to:
let nested = match maybe.get("key") {
    Some(v) => v.nested_field,
    None => None,
}
```

### 5. Indentation is the syntax

JavaScript uses braces. Buff uses indentation (like Python). Mixing
tabs and spaces is a hard lexer error — set your editor to "insert
spaces for tabs" and set the width to 4. `buff fmt` enforces this.

```javascript
// JavaScript
if (x > 0) {
    console.log("positive");
}
```

```buff
// Buff
if x > 0:
    print("positive")
```

Bonus pitfall: Buff's `if`/`else if`/`else` chains use indent blocks,
not braces. The `else` keyword goes at the same indent as the
matching `if`:

```buff
if x > 0:
    print("positive")
else if x < 0:
    print("negative")
else:
    print("zero")
```

## Where to go next

1. **Install Buff**: [Getting Started → Installation](../getting-started/installation/).
2. **First program**: [Getting Started → First program](../getting-started/first-program/).
3. **Skim the syntax**: [Language → Syntax](../language/syntax/).
4. **Browse the cookbook**: [Cookbook](../cookbook/_index/) — 55
   recipes. The [HTTP](../cookbook/http/) and
   [Async](../cookbook/async/) pages are the closest analogues to
   Express/Promise patterns.
5. **Try `.buffhtml`**: [`.buffhtml` examples](https://github.com/buff-lang/buff/tree/master/examples)
   — `counter.buffhtml`, `todo_list.buffhtml`, `lifecycle_demo.buffhtml`,
   `typed_props.buffhtml`, `composition_demo.buffhtml`, `todo_app.buffhtml`.
   These are the closest analogues to React/Vue SFCs.
6. **Browse the frameworks**: [Frameworks → Overview](../frameworks/overview/)
   — every `buff-*` crate, including `buff-web` (v1.15+, the Express
   equivalent) and `buff-ui-dioxus` (the React/Vue equivalent).
7. **Try the LSP**: [VSCode extension](https://github.com/buff-lang/buff/tree/master/editors/vscode)
   bundles `buff-lsp` for hover, completion, goto-definition,
   formatting on save.

If you get stuck, file an issue in the [buff] repo — the onboarding
guides are tracked by T69 and updated as the language evolves.

[buff]: https://github.com/buff-lang/buff
