# Introduction

Welcome to **The Buff Book**.

Buff is a high-level language that transpiles to Rust, which is then compiled
to a native binary by `rustc` / LLVM. The pitch in one sentence:

> **Rust performance with Go productivity.** Write clean, indentation-based
> code and get a binary that fans out across CPU cores and dispatches hot loops
> to the GPU — without writing a single lock, thread, shader, or lifetime
> annotation.

Buff exists to break a painful trilemma that has governed language choice for a
decade:

| You want… | You pick… | You pay… |
|---|---|---|
| Maximum performance & memory safety | **Rust** | A brutal learning curve — fighting the borrow checker, annotating lifetimes |
| Simplicity & productivity | **Go / C# / Java** | A garbage collector: pauses, extra RAM, hidden overhead |
| Both | — | *"The Holy Grail"* — supposedly impossible |

Buff's bet is that the trilemma is *artificial*. Three ideas make it work:

1. **Transpile, don't reimplement.** Buff is a source-to-source compiler
   (`.buff` → Rust → native). It piggybacks on the engineering sunk into
   `rustc` instead of reinventing codegen. The borrow checker becomes a *free
   safety reviewer* of generated code, never an obstacle the user sees.
2. **Hide memory management.** No references (`&`), no visible lifetimes
   (`'a`), no manual pointers in Buff syntax. The transpiler emits only "easy"
   Rust — owned data, intelligent clones, `Arc` / copy-on-write where sharing
   is needed.
3. **Invisible heterogeneous computing.** The same Buff function can run on
   CPU *or* dispatch to GPU automatically — the compiler analyzes arithmetic
   intensity and emits both a Rayon path **and** a WGSL shader, then the runtime
   picks at execution time. Hints like `@prefer(gpu)` nudge the decision but
   **never break** when hardware is absent.

## Who this book is for

This book assumes you can read code in at least one C-family language (C, C++,
Java, C#, JavaScript, TypeScript, Python, Go, or Rust). No Rust background is
required — *Chapter 9* includes a dedicated "coming from Rust" migration guide
for readers who do know Rust and want to see exactly what friction Buff removes.

If you have never programmed, this book will move quickly; pair it with a
beginner resource in a language you already know. If you are an experienced
systems engineer evaluating Buff for a production codebase, you can safely skip
to *Chapter 6 (Language Reference)* and *Chapter 8 (Error Code Handbook)* and
treat the earlier chapters as walkthroughs.

## How to read this book

The book has three parts:

- **Foundations** *(Chapters 1–3)* — install Buff, write your first program,
  then build three real artifacts: a CLI tool, an HTTP API server. By the end
  you can ship a useful Buff program.
- **Advanced Capabilities** *(Chapters 4–5)* — the two features that make Buff
  unlike Go or Python: invisible GPU compute, and a `.buffhtml` single-file
  component model for building user interfaces that compile to WebAssembly.
- **Reference** *(Chapters 6–9)* — the language grammar, the standard library,
  every compiler error code (`E10xx`–`E15xx`), and side-by-side migration
  guides from Rust, Python, and Go.

Read the Foundations chapters in order. Treat the Advanced and Reference
chapters as dip-in material.

## Conventions

A snippet like this is Buff source you can save to a file and run:

```buff
func main():
    print("Hello, Buff!")
```

Run it with:

```bash
buff run hello.buff
```

Throughout the book you will see two status markers next to example headings:

- 🟢 **runs** — `buff run <file>` compiles and executes end-to-end today.
- 🔶 **type-checks** — the snippet is valid Buff and passes `buff check`, but
  end-to-end execution depends on a sibling codegen task still in flight.

Both kinds of snippet are real Buff syntax. The 🔶 marker is honest about which
features are wired through the full pipeline *today* versus which are
codegen-verified and pending a coordinated sibling commit (Buff ships under a
strict "the example must compile" rule — see the root [`README.md`][root-readme]
status table).

[root-readme]: https://github.com/buff-lang/buff/blob/v1x-frameworks/README.md

## The 25 keywords

Buff reserves 25 keywords. Everything else is an identifier:

```
func let mut struct enum trait type if else for return break continue in match
async spawn import export from as true false extern unsafe
```

Notably **absent**: `class`, inheritance, `null` / `nil`, manual pointers
(`*` `&`), visible lifetimes (`'a`), `await`, `try` / `catch`. *Chapter 6*
covers each of these absences and what Buff does instead.

## A note on "the compiler does it for you"

You will read this phrase often. It is the whole point of Buff. When the book
says *"the compiler inserts the `.await` for you,"* it means: the Buff
front-end (lexer → parser → type-inference) analyzed your program, decided a
`.await` is needed at this call site, and emitted Rust source that already
contains it. You never typed `.await`. You never will.

This pattern — *analyze, decide, emit easy Rust* — repeats for memory
management (clones, `Arc`), GPU dispatch (WGSL shaders), numeric-width
narrowing, async propagation, and more. The result is source code that reads
like Python but produces binaries that run like Rust.

## Let's go

Turn the page to [Chapter 1 — Getting Started](./chapter-1.md).
