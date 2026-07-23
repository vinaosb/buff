+++
title = "Buff for Python developers"
weight = 46
+++

# Buff for Python developers

> Python is the most popular teaching language on Earth because it
> reads like English. Buff keeps that readability — same indentation
> blocks, same `func`-then-body shape, same lack of `class` boilerplate
> for simple cases — and adds static typing, native performance, and
> transparent parallelism without sacrificing the `print("hello")`
> ergonomics. If you've written a `for x in xs` loop in Python, you
> already know most of Buff's surface.

This guide maps the Python you already know onto Buff's syntax,
tooling, and ecosystem. It assumes you can read Python 3.10+ and have
seen `async`/`await`, `typing`, and `pandas`. You'll be productive in
Buff in under an hour.

## Why Buff?

For most Python developers, the value prop of Buff is straightforward:

1. **Native performance without Cython.** Buff transpiles to Rust and
   compiles to a native binary via `rustc` + LLVM. There's no GIL, no
   interpreter startup, no `pyproject.toml` zoo. The same code that
   does CPU-bound work in Python at 1x runs at 50-100x in Buff.
2. **Real static types without the `typing` tax.** Buff infers types
   aggressively — you rarely write them. But every variable is typed
   at compile time, and type mismatches are compile errors, not
   runtime surprises.
3. **Parallelism that just works.** Python's `multiprocessing` is
   awkward; `asyncio` is everywhere but single-threaded. Buff's
   `par_map` runs across all your CPU cores by default (via Rayon),
   and the same loop can be transparently dispatched to the GPU via
   `@prefer(gpu)` — no `numba`, no `cuda`, no wheel-build pain.
4. **No borrow checker fights.** Rust gives you the same speed but
   forces you to argue with it about ownership. Buff's compiler
   inserts the right `clone()` calls and `Arc`s for you.

The trade-off: Buff is younger than Python, so the ecosystem is
smaller. If you need a niche scientific library (`astropy`,
`biopython`), Python still wins. For pure-compute, CLI, server, and
data-pipeline work, Buff is competitive today and growing fast.

## Syntax mapping table

The fastest way to ramp up. Read the left column, see the right column,
internalize it.

### Fundamentals

| Python | Buff | Notes |
|---|---|---|
| `def f():` | `func f():` | `def` → `func`. Identical body shape. |
| `print("hi")` | `print("hi")` | Same. Buff's `print` is in the prelude. |
| `# comment` | `// comment` | Buff has no block comments — write multiple `//`. |
| `x = 5` | `let x = 5` | `let` is required (immutable by default). |
| `x: int = 5` | `let x: Int = 5` | Annotation is optional in both; inferred in Buff. |
| `x = 5; x = 6` | `let mut x = 5; x = 6` | `mut` opts into mutation. |
| `True` / `False` | `true` / `false` | Lowercase. |
| `None` | `None` | Same keyword, but typed `Option<T>`. |
| `pass` | (just omit) | Empty blocks aren't allowed; omit the body or use a comment. |
| 4-space indent | 4-space indent | Tabs are a lexer error in Buff. |

### Strings, numbers, and printing

| Python | Buff | Notes |
|---|---|---|
| `f"name: {name}"` | `"name: " + name` (concat) | Buff has no f-strings or interpolation yet — use `+` and `print(a, b, c)`. |
| `len(s)` | `s.len()` | Method on the string. |
| `s.upper()` | `s.to_uppercase()` | Different method names. |
| `s.split(",")` | `s.split(",")` | Same. |
| `s.strip()` | `s.trim()` | `strip` → `trim`. |
| `"x" * 3` | (no equivalent — use a loop) | No string repetition operator. |
| `1_000_000` | `1_000_000` | Underscore separator works in both. |
| `3.14` | `3.14` | Float. |
| `10 / 3` (float) | `10 / 3` | Float division. |
| `10 // 3` | `10 / 3` (int types) | Integer types do integer division; no separate `//`. |
| `2 ** 10` | `Math.pow(2, 10)` | `**` is not an operator in Buff — use `Math.pow` or `x * x`. |

### Collections

| Python | Buff | Notes |
|---|---|---|
| `[1, 2, 3]` | `[1, 2, 3]` | List → `Vector<T>`. |
| `list.append(x)` | `v.push(x)` | `append` → `push`. |
| `list.pop()` | `v.pop()` | Same; returns `Option<T>`. |
| `len(list)` | `v.len()` | Method, not function. |
| `list[i]` | `v[i]` | Same. |
| `[x*2 for x in xs]` | `xs.map({ x => x * 2 })` | List comprehension → `.map()`. |
| `[x for x in xs if x>0]` | `xs.par_filter({ x => x > 0 })` | Parallel filter. |
| `sum(xs)` | `xs.reduce({ a, b => a + b })` | Or `par_reduce` for parallel. |
| `dict()` / `{1: "a"}` | `{1: "a"}` | Dict → `Map<K, V>`. |
| `d[k]` | (use `d.get(k)`) | Direct index lookup on `Map<K,V>` is a codegen gap. |
| `d.get(k, default)` | `match d.get(k) { Some(v) => v, None => default }` | Or `?? default`. |
| `tuple` | (no equivalent) | Buff has no tuples. Use a struct. |

### Control flow

| Python | Buff | Notes |
|---|---|---|
| `if x > 0: ...` | `if x > 0:` | Same shape. |
| `elif` | `else if` | Two words. |
| `for x in xs:` | `for x in xs:` | Same. |
| `for i in range(10):` | `for i in 0..10:` | Range syntax. Inclusive: `0..=10`. |
| `while x < 10:` | (use `for` over an iterator, or recursion) | Buff has no `while`. |
| `match x:` | `match x { ... }` | Rust-style, braces required. |
| `match` cases | `case Pat:` | `Pat => body,` |
| `break` / `continue` | `break` / `continue` | Same. |
| `try: ... except E as e:` | `match f() { Ok(v) => ..., Err(e) => ... }` | No `try`/`except`; use `Result<T, E>`. |
| `raise ValueError(...)` | `return Error("...")` | Returns a `Result<T, Error>`. |
| `with open(p) as f:` | (no `with`) | Use `Path.open()` or `Tempfile.new()`; cleanup is automatic. |
| `lambda x: x * 2` | `{ x => x * 2 }` | Lambda syntax. |

### Functions and types

| Python | Buff | Notes |
|---|---|---|
| `def f(x, y):` | `func f(x, y):` | Param types inferred from callers in MVP. |
| `def f(x: int) -> int:` | `func f(x: Int) -> Int:` | Same shape. |
| `def f(*args, **kwargs):` | (no equivalent) | Use a struct or `Vector<T>`. |
| `def f(x="default"):` | `func f(x: String = "default"):` | Default args supported. |
| `@staticmethod` / `@classmethod` | (free function / `Type.method()`) | No method kinds; use module fns or `impl`. |
| `@dataclass` | `struct` | `struct` is a data class by default — no decorator. |
| `@property` | (use a method) | No getters; expose methods. |
| `class Foo(Bar):` | (no inheritance) | Use `trait` + composition. |
| `from typing import Optional` | `Option<T>` | Same idea. |
| `from typing import List` | `Vector<T>` | Built-in. |
| `from typing import Dict` | `Map<K, V>` | Built-in. |
| `Callable[[int], int]` | `fn(Int) -> Int` (conceptual) | Use `trait Fn(Int) -> Int` or generics. |

### Async

| Python | Buff | Notes |
|---|---|---|
| `async def f():` | `async func f():` | Same idea. |
| `await f()` | `f()` | **No `await` keyword.** Async propagates up the call graph. |
| `asyncio.gather(*tasks)` | `spawn` + `task.result()` | Or use `Channel<T>` for producer/consumer. |
| `asyncio.sleep(s)` | `sleep(Duration.seconds(s))` | Same semantics. |
| `asyncio.Lock()` | (runtime-managed) | Buff hides the locks; you don't write them. |
| `asyncio.Queue()` | `Channel<T>` | MPSC primitive. |
| `@asyncio.coroutine` (legacy) | (n/a) | No legacy coroutines. |

### Decorators

| Python | Buff | Notes |
|---|---|---|
| `@property` | (use a method) | No getters/setters. |
| `@staticmethod` | (free function) | Module-level functions. |
| `@dataclass` | `struct Foo:` | `struct` is already a data class. |
| `@functools.lru_cache` | `@Cached let x = ...` | Property wrapper. |
| `@pytest.fixture` | (use a `func` returning the value) | No fixtures; write plain functions. |
| `@functools.wraps` | (n/a) | No reflection-based decorators. |
| `@app.route("/x")` | (use `buff_web`) | Different framework. |
| `@test` (pytest convention) | `@test` attribute | Marks a function as a test for `buff test`. |

## Tooling migration

The Python toolchain is sprawling; Buff's is one binary. The table
maps each Python tool to its Buff equivalent.

| Python | Buff | Notes |
|---|---|---|
| `python` (interpreter) | `buff run <file>` | Buff compiles then runs; there is no REPL-on-every-file. |
| `python -m venv` | (not needed) | Buff has no virtual envs — every project is its own directory. |
| `pip install pkg` | `buff add pkg` | Adds a dependency to `buff.toml`. |
| `pip install -r requirements.txt` | `buff deps` | Resolves `buff.toml` and clones dependencies. |
| `poetry` / `pdm` / `uv` / `hatch` | `buff` (built-in) | One tool, no plugin zoo. |
| `pip install --upgrade pkg` | `buff update pkg` | Updates a single dep. |
| `pip list --outdated` | `buff outdated` | Lists outdated deps. |
| `pip freeze > requirements.txt` | (auto-tracked in `buff.lock`) | Lockfile is gitignored by default. |
| `pytest` | `buff test` | Discovers `@test` functions and runs them. |
| `black` / `autopep8` / `yapf` / `isort` | `buff fmt` | One formatter, no config debates. |
| `flake8` / `pylint` / `ruff` | `buff check` | Type-checker + linter combined. |
| `mypy` / `pyright` | `buff check` | Statically types without running. |
| `pyinstaller` / `cx_Freeze` | `buff build` | Produces a native binary. |
| `pyinstaller --onefile` | `buff build --minimal` | Strips the binary (often <1MB). |
| `python -m http.server` | (use `buff_web`) | Different framework. |
| `pypi.org` (registry) | `buff-registry` | Self-hosted or community instance. |
| `setuptools` / `setup.py` | `buff.toml` | One declarative file. |
| `twine upload` | `buff publish` | Publishes to the registry. |
| `.python-version` / `pyenv` | `buffup` | Version manager (v1.12). |
| `pre-commit` | (use git hooks manually) | No built-in hook orchestrator. |
| `tox` | (use CI matrix) | Buff's CI runs the same commands on 3 OSes. |
| `Jupyter` | `buff jupyter` | Pure-Rust ZMQ kernel (v1.7). |

### Build vs run

In Python, you `python file.py` and the interpreter runs the file
top-to-bottom. In Buff, there's a compile step in between:

```bash
buff run examples/fibonacci.buff      # compile + run, throw away binary
buff build examples/fibonacci.buff    # compile, keep binary at ./fibonacci
buff check examples/fibonacci.buff    # type-check only, no codegen (fast)
```

`buff check` is what your editor runs on save. `buff run` is for
development. `buff build` is for distribution.

### The project layout

A new Buff project (`buff new my_app`) looks like:

```
my_app/
├── buff.toml          # project manifest (like pyproject.toml)
├── src/
│   └── main.buff      # entry point (like __main__.py)
└── tests/
    └── test_main.buff # test file (like test_main.py)
```

Compare to a Python `pyproject.toml`-based project:

```
my_app/
├── pyproject.toml
├── src/
│   └── my_app/
│       └── __init__.py
└── tests/
    └── test_main.py
```

The shape is the same. The differences:

- Buff has one entry point (`src/main.buff`), not a package.
- `buff.toml` is one file with no plugin ecosystem (no `setuptools`,
  no `poetry-core`, no `hatchling`).
- There is no `__init__.py` — Buff modules are files, and you
  `import` them by path.

### Dependency declaration

In `pyproject.toml`:

```toml
[project]
dependencies = [
    "requests>=2.31",
    "pandas>=2.0",
]
```

In `buff.toml`:

```toml
[deps]
buff_http_client = "1.0"
buff_dataframe = "1.0"
```

Buff dependencies are themselves Buff packages published to a
`buff-registry` instance. You can also depend on Rust crates directly
via the `[rust-deps]` section when wrapping them through `extern`
(see the Rust developer guide for details).

## Ecosystem mapping

The Python ecosystem is enormous. Buff doesn't try to clone it
one-to-one; instead, common Python libraries map onto either a
stdlib prelude type, a `buff-*` framework crate, or a Rust crate
reachable through `extern` FFI.

| Python library | Buff equivalent | Notes |
|---|---|---|
| `requests` / `httpx` | `HttpClient` (prelude) | Wraps `reqwest`. |
| `aiohttp` | `HttpClient` (async) | Same surface; async is automatic. |
| `urllib` | `URL` (prelude) | Parse + manipulate URLs. |
| `json` | `Toml.parse` / `Json` (prelude) | JSON is parsed into a `Map<String, String>`. |
| `csv` | `Csv.parse` (prelude) | CSV reader. |
| `toml` / `tomllib` | `Toml` (prelude) | TOML is Buff's preferred config format. |
| `yaml` | `Yaml` (prelude) | YAML reader. |
| `re` | `Regex` (prelude) | Wraps the `regex` crate. |
| `pathlib.Path` | `Path` / `Dir` (prelude) | Filesystem paths. |
| `os` / `shutil` | `Filesystem` / `Process` / `Env` (prelude) | OS interaction. |
| `tempfile` | `Tempfile` (prelude) | Auto-cleanup temp files. |
| `datetime` | `DateTime` (prelude) | Time, dates, durations. |
| `time.sleep` | `sleep(Duration.seconds(N))` | Async-aware sleep. |
| `math` | `Math` (prelude) | `pow`, `sqrt`, `sin`, `cos`, etc. |
| `random` | `Random` (prelude) | Wraps the `rand` crate. |
| `hashlib` | `Hash` / `HMAC` (prelude) | SHA-256, HMAC. |
| `base64` | `Base64` (prelude) | Encode/decode. |
| `uuid` | `UUID` (prelude) | UUIDv4 generation. |
| `secrets` | `Random.secure_bytes(N)` | CSPRNG. |
| `subprocess` | `Process.run(cmd)` | Spawn external processes. |
| `socket` | `TCP` / `UDP` (prelude) | Network primitives. |
| `websockets` | `WebSocket` (prelude) | Async WebSocket client. |
| `logging` | `Log` (prelude) | Structured logging. |
| `argparse` | `Args` (prelude) | CLI argument parsing. |
| `pandas` | `buff-dataframe` (v1.13+) | See DataFrame section below. |
| `numpy` | `buff-tensor` (v1.13+) | N-dimensional arrays. |
| `flask` / `fastapi` | `buff-web` (v1.15+) | Web framework. |
| `sqlalchemy` | `buff-db` (v1.15+) | Database ORM/query builder. |
| `jinja2` | `buff-template` (v1.15+) | HTML templating. |
| `celery` | `buff-jobs` (v1.16+) | Background job queue. |
| `redis` | `buff-cache` (v1.16+, in-memory MVP) | Distributed cache. |
| `boto3` | (use Rust AWS SDK via `extern`) | No native AWS SDK yet. |
| `psycopg2` | `buff-db` with Postgres | Wraps a pure-Rust driver. |
| `pytest` | `buff test` (built-in) | Test runner is built-in. |
| `pytest-mock` | `buff-mock` (v1.13+) | Mocking framework. |
| `hypothesis` | `@test` + property attributes | Property-based testing. |
| `pydantic` | `buff-validate` (v1.16+) | Runtime data validation. |
| `click` / `typer` | `Args` (prelude) | CLI framework is built-in. |

If a library you need isn't in this list, search the [frameworks
overview](../frameworks/overview/) — the v1.x roadmap ships ~30
`buff-*` crates. For anything not covered, you can always `extern`
the underlying Rust crate (see the [Rust developer guide](./rust-developers/)).

## Hello World, side by side

A canonical first program. Print a greeting, count to three, do a
tiny calculation.

### Python

```python
import sys

def greet(name: str) -> str:
    return f"Hello, {name}!"

def main():
    for i in range(1, 4):
        print(f"count: {i}")
    who = sys.argv[1] if len(sys.argv) > 1 else "World"
    print(greet(who))
    print(f"2 + 2 = {2 + 2}")

if __name__ == "__main__":
    main()
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

- **`import sys`** → gone. `Args` is in the prelude; no import needed.
- **`def greet(name: str) -> str`** → `func greet(name: String) -> String`.
  `def` → `func`; `str` → `String` (capital S — types are PascalCase in
  Buff).
- **`f"Hello, {name}!"`** → `"Hello, " + name + "!"`. Buff has no
  f-strings yet. Use `+` concatenation and the `print(a, b, c)`
  multi-arg form. (Interpolation is a planned feature.)
- **`for i in range(1, 4)`** → `for i in 1..=3`. The range syntax is
  `start..end` (exclusive) or `start..=end` (inclusive).
- **`f"count: {i}"`** → `"count: " + i.string()`. Integers aren't
  auto-converted to strings; call `.string()` explicitly. (This is
  one of the bigger Python→Buff mental shifts; see pitfalls below.)
- **`sys.argv[1]`** → `Args.all()[1]`. The prelude `Args` module wraps
  the OS args. Note `let args = Args.all()` — Buff requires `let`.
- **`if len(args) > 1 { ... } else { ... }`** → Buff's `if` is an
  expression when wrapped in braces. No ternary `x if cond else y`
  syntax.
- **`if __name__ == "__main__":`** → gone. `func main():` is the entry
  point; the compiler emits the C-level entry for you.

## Async without `await`

The single biggest Python→Buff mental shift is async. In Python:

```python
import asyncio

async def fetch_user(uid):
    await asyncio.sleep(0.1)
    return {"id": uid}

async def main():
    a = await fetch_user(1)
    b = await fetch_user(2)
    print(a, b)

asyncio.run(main())
```

Every `async def` function must be `await`ed at the call site. Forgetting
`await` is a common bug — you get a coroutine object instead of the
value. Python's "function coloring" problem.

In Buff:

```buff
async func fetch_user(uid: Int) -> Map<String, String>:
    sleep(Duration.millis(100))
    return {"id": uid.string()}

func main():
    let a = fetch_user(1)
    let b = fetch_user(2)
    print(a, b)
```

Notice:

1. **No `await` keyword.** You call `fetch_user(1)` like any other
   function.
2. **`main` is not declared `async`.** The compiler sees that `main`
   calls an async function and propagates async-ness upward
   automatically, then emits `#[tokio::main]` on the generated Rust.
3. **`sleep(...)` is also async.** But you don't write `await sleep(...)`
   — Buff inserts the `.await` for you.

You only opt into async when you want to. Most code is synchronous and
has no idea async exists underneath.

For concurrent execution (run two fetches in parallel):

```buff
func main():
    let task_a = spawn fetch_user(1)
    let task_b = spawn fetch_user(2)
    let a = task_a.result()
    let b = task_b.result()
    print(a, b)
```

`spawn` schedules a task on the tokio runtime. `task.result()` blocks
the caller until the task completes and returns its value. This is the
Buff equivalent of Python's `asyncio.gather(fetch_user(1), fetch_user(2))`.

See the [Async cookbook](../cookbook/async/) for the full pattern
catalog (timeout, `select`, gather, channels).

## Type hints vs type inference

Python's `typing` module is opt-in and runs at... well, never. `mypy`
checks types statically but the interpreter ignores them at runtime.
Buff has no "runtime" types — types are erased to native machine
representation by the time the binary runs. The compiler checks them
once, at compile time.

The mental shift:

| Python | Buff |
|---|---|
| `x: int = 5` | `let x: Int = 5` (or just `let x = 5`) |
| `List[int]` | `Vector<Int>` |
| `Dict[str, int]` | `Map<String, Int>` |
| `Optional[int]` | `Option<Int>` |
| `Tuple[int, str]` | (no tuples — use a struct) |
| `Callable[[int], int]` | (use `trait Fn(Int) -> Int`) |
| `Union[int, str]` | (no unions — use an enum) |
| `Any` | (forbidden — use generics) |
| `TypeVar("T")` | `T` (just use the name) |
| `Generic[T]` | `trait Box<T>: ...` |
| `@overload` | (no overloading — use multiple dispatch T58 or default args) |
| `Protocol` | `trait` |
| `TypedDict` | `struct` |

Buff's inference is aggressive. The following code has no type
annotations but is fully typed:

```buff
func main():
    let nums = [1, 2, 3]               // Vector<Int>
    let doubled = nums.map({ x => x * 2 })  // Vector<Int>
    let total = doubled.reduce({ a, b => a + b }, 0)  // Int
    print(total)                       // 12
```

The compiler infers `Vector<Int>` from the integer literals, `Int`
for the closure parameter from the `.map()` signature, and `Int` for
the accumulator. You'd write annotations only at function boundaries
(public APIs) and when the inference is ambiguous.

## DataFrame vs pandas

`buff-dataframe` (v1.13 frameworks wave 2) is Buff's answer to
`pandas`. The API surface is intentionally smaller — pandas has 20
years of accreted methods; Buff starts fresh.

| pandas | Buff DataFrame | Notes |
|---|---|---|
| `import pandas as pd` | `import buff_dataframe` (or prelude) | DataFrame is in the prelude from v1.13. |
| `pd.read_csv("f.csv")` | `DataFrame.load_csv("f.csv")` | Same idea. |
| `pd.read_json("f.json")` | `DataFrame.load_json("f.json")` | Wraps `serde_json`. |
| `df.head(5)` | `df.head(5)` | Same. |
| `df.shape` | `df.rows()` + `df.cols()` | Tuple → two methods (no tuples in Buff). |
| `df["col"]` | `df.column("col")` | Method, not indexing. |
| `df[df["age"] > 18]` | `df.filter({ row => row.age > 18 })` | Closure-based filter. |
| `df.groupby("city")` | `df.group_by("city")` | Snake_case in Buff. |
| `df.merge(other, on="id")` | `df.join(other, on: "id")` | Named arg for `on`. |
| `df.sort_values("age")` | `df.sort_by("age")` | Same idea. |
| `df.to_csv("out.csv")` | `df.write_csv("out.csv")` | Same. |
| `df.apply(np.sqrt)` | `df.map({ x => Math.sqrt(x) })` | Closure-based. |
| `df.agg(["mean", "sum"])` | `df.aggregate(["mean", "sum"])` | Same idea. |
| `pd.concat([df1, df2])` | `df1.concat(df2)` | Method on the first. |
| `df.dropna()` | `df.drop_na()` | Same. |
| `df.fillna(0)` | `df.fill_na(0)` | Same. |

A canonical pandas→Buff translation:

```python
# pandas
import pandas as pd

df = pd.read_csv("users.csv")
adults = df[df["age"] >= 18]
by_city = adults.groupby("city")["age"].mean()
print(by_city.sort_values(ascending=False).head(10))
```

```buff
// Buff
func main():
    let df = DataFrame.load_csv("users.csv")
    let adults = df.filter({ row => row.age >= 18 })
    let by_city = adults.group_by("city").mean("age")
    let sorted = by_city.sort_by("age_mean", descending: true)
    print(sorted.head(10))
```

See the [DataFrame cookbook](../cookbook/dataframe/) for the full
recipe set: load, filter, group, join, export, schema.

## List comprehensions to `par_map`

Python list comprehensions are idiomatic and fast (in CPython terms).
Buff's equivalent is the `.map()` / `.par_map()` combinator on
`Vector<T>`.

```python
# Python
squares = [x * x for x in range(1, 11)]
evens = [x for x in squares if x % 2 == 0]
total = sum(evens)
```

```buff
// Buff (sequential)
let squares = (1..=10).map({ x => x * x })
let evens = squares.filter({ x => x % 2 == 0 })
let total = evens.reduce({ a, b => a + b }, 0)
```

```buff
// Buff (parallel — uses all CPU cores)
let squares = (1..=10).par_map({ x => x * x })
let evens = squares.par_filter({ x => x % 2 == 0 })
let total = evens.par_reduce({ a, b => a + b }, 0)
```

The parallel versions are drop-in replacements. For large vectors,
`par_map` scales nearly linearly with core count. The same loop can
also be dispatched to GPU via the `@prefer(gpu)` attribute (see
[Language → Attributes](../language/attributes/)).

## Decorators to `@attribute` and property wrappers

Python decorators are functions that wrap other functions. Buff
attributes (`@test`, `@prefer(gpu)`, `@comptime`, `@deprecated`, etc.)
are **compiler hints** — they're not runtime wrappers. For the
equivalent of a Python decorator, you write a function explicitly:

```python
# Python
import time
from functools import wraps

def timed(fn):
    @wraps(fn)
    def wrapper(*args, **kwargs):
        start = time.time()
        result = fn(*args, **kwargs)
        print(f"{fn.__name__}: {time.time() - start:.3f}s")
        return result
    return wrapper

@timed
def slow():
    time.sleep(1)
    return 42
```

```buff
// Buff — no decorator system; use a higher-order function explicitly.
func timed<T>(fn: func() -> T) -> T:
    let start = DateTime.now()
    let result = fn()
    let elapsed = DateTime.now() - start
    print("elapsed: " + elapsed.string())
    return result

func slow() -> Int:
    sleep(Duration.seconds(1))
    return 42

func main():
    let result = timed(slow)
    print(result)
```

For stateful wrapping (caching, memoization), Buff ships **property
wrappers** (T56, Swift-inspired) — `@State`, `@Cached`, `@Published`,
`@Observed`. These desugar at parse time to a reactive cell:

```buff
@Cached let fib_table = compute_fib_table()
@State let count = 0

count.set(5)
print(count.get())
```

See the [Attributes reference](../language/attributes/) for the full
list of built-in attributes and what they do.

## `with` statements → automatic resource management

Python's `with` statement guarantees cleanup (file close, lock release)
even on exceptions:

```python
# Python
with open("data.txt") as f:
    contents = f.read()
# f is closed here, even if read() raised
```

Buff has no `with` keyword. Instead, fallible resources (files,
sockets, temp files) implement RAII — they're cleaned up automatically
when the binding goes out of scope. The Buff equivalent:

```buff
func main():
    let f = Path.open("data.txt")
    let contents = f.read()
    // f is closed when the function returns; you don't manage it.
    print(contents)
```

For temp files specifically, the `Tempfile` prelude type guarantees
deletion on drop:

```buff
func main():
    let tmp = Tempfile.new()
    tmp.write("scratch data")
    // tmp is deleted from disk when this function returns.
```

This matches Rust's RAII model — Buff inherits it. The compiler emits
the equivalent of Python's `with` block at the generated Rust layer,
so you never write it manually.

## f-strings → string interpolation

Python's f-strings are ergonomic. Buff has not yet shipped
interpolation syntax (it's on the roadmap). Until it does, use
concatenation and `print(a, b, c)`:

```python
# Python
name = "Alice"
age = 30
print(f"{name} is {age} years old")
```

```buff
// Buff
let name = "Alice"
let age = 30
print(name + " is " + age.string() + " years old")
// Or pass multiple args:
print(name, "is", age, "years old")
```

The `.string()` method is the equivalent of Python's `str(x)`. It
exists on every type that implements the `Display` trait.

## pip → buff add / cargo

Dependency management in Python is famously fragmented (`pip` +
`setuptools`, `poetry`, `pdm`, `uv`, `hatch`, `pipenv`, `conda`,
...). Buff has one tool: `buff`.

```bash
# Python
pip install requests
pip install -r requirements.txt
pip freeze > requirements.txt

# Buff
buff add buff_http_client       # add a dep, write to buff.toml
buff deps                        # resolve + download all deps
# buff.lock is auto-generated (gitignored)
```

For Rust crates that don't have a Buff wrapper yet, you can declare
them as direct Rust dependencies and bind them via `extern`:

```toml
# buff.toml
[rust-deps]
reqwest = "0.12"
tokio = "1.40"
```

```buff
extern "C" from "reqwest" func fetch_text(url: String) -> String
```

See the [extern FFI guide](https://github.com/buff-lang/buff/blob/master/docs/extern-guide.md)
and the [Rust developer guide](./rust-developers/) for the full FFI
story.

## Common pitfalls

The five things that trip up Python developers most often:

### 1. Forgetting `let`

In Python, `x = 5` introduces a new variable. In Buff, you must write
`let x = 5` for the first assignment and `x = 5` for re-assignment.

```buff
let count = 0         // first assignment — must use `let`
count = count + 1     // re-assignment — no `let`

let mut total = 0     // mutable binding
total = total + 5     // OK
```

Forgetting `let` gives a parse error, not a runtime NameError. The
error message points at the missing keyword.

### 2. Forgetting `mut`

`let x = 5` is immutable. Re-assigning it is a compile error:

```buff
let x = 5
x = 10                 // ERROR: cannot assign to immutable binding
```

Use `let mut x = 5` to opt into mutation. This is the same rule as
Rust. Methods that mutate (`push`, `pop`, `set`) require a `mut`
binding.

### 3. Numbers don't auto-stringify

In Python, `f"x = {n}"` works for any `n`. In Buff, you must call
`.string()` explicitly:

```buff
let n = 42
print("x = " + n)             // ERROR: cannot concatenate String and Int
print("x = " + n.string())    // OK
print("x =", n)               // OK — print accepts multiple args
```

This catches bugs (accidentally printing a vector full of pointers)
at the cost of a `.string()` call. The `.string()` method exists on
every printable type.

### 4. No `try`/`except`

In Python, you wrap fallible code in `try`/`except`. In Buff, every
fallible function returns a `Result<T, E>` and you handle it explicitly:

```python
# Python
try:
    value = int(s)
except ValueError:
    value = 0
```

```buff
// Buff
let value = match s.parse_int() {
    Ok(n) => n,
    Err(_) => 0,
}
// Or with the ?? null-coalesce operator:
let value = s.parse_int() ?? 0
```

There's no global exception handler. Errors are values. This matches
Rust and Go; the pitfall is assuming you can `try` your way out of a
bad state.

### 5. Indentation errors are syntax errors

In Python, mixing tabs and spaces is a `TabError` at runtime. In Buff,
**tabs are a hard lexer error** — the file won't parse at all. Set
your editor to "insert spaces for tabs" and set the tab width to 4.
The Buff formatter (`buff fmt`) enforces this; running it once on
your code is the simplest fix.

Bonus: Buff also rejects trailing whitespace and more than two
consecutive blank lines. `buff fmt` cleans these up automatically.

## Where to go next

You've read the guide. Now:

1. **Install Buff** if you haven't: [Getting Started → Installation](../getting-started/installation/).
2. **Run your first program**: [Getting Started → First program](../getting-started/first-program/).
3. **Skim the syntax reference**: [Language → Syntax](../language/syntax/).
4. **Browse the cookbook**: pick a recipe closest to what you want
   to build — [HTTP](../cookbook/http/), [Files](../cookbook/files/),
   [JSON](../cookbook/json/), [Database](../cookbook/database/),
   [Parallel](../cookbook/parallel/), [Async](../cookbook/async/),
   [Errors](../cookbook/errors/), [Testing](../cookbook/testing/),
   [Strings](../cookbook/strings/), [DataFrame](../cookbook/dataframe/).
5. **Read the examples**: [`examples/`](https://github.com/buff-lang/buff/tree/master/examples)
   has runnable `.buff` files for every major feature. The
   [`examples/closures.buff`](https://github.com/buff-lang/buff/blob/master/examples/closures.buff)
   and
   [`examples/collections.buff`](https://github.com/buff-lang/buff/blob/master/examples/collections.buff)
   are the closest analogues to a pandas-or-numpy workflow.
6. **Browse the frameworks**: [Frameworks → Overview](../frameworks/overview/)
   lists every `buff-*` crate. The v1.x roadmap ships ~30 of them.
7. **Use the LSP**: install the [VSCode extension](https://github.com/buff-lang/buff/tree/master/editors/vscode)
   for hover, completion, and goto-definition. It bundles `buff-lsp`.

If you get stuck, file an issue in the [buff] repo — the onboarding
guides are tracked by T69 and updated as the language evolves.

[buff]: https://github.com/buff-lang/buff
