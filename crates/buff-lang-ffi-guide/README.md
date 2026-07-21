# buff-lang-ffi-guide

Documentation-only crate defining hard rules for `extern` FFI usage in Buff frameworks.

## What this is

When Buff frameworks wrap Rust libraries (reqwest, serde, regex, tokio, etc.), the boundary between Rust's memory model and Buff's managed view needs strict rules. This crate holds those rules.

## What this is not

This crate contains no production code. It will never export a function, struct, or trait. Its purpose is to serve as a permanent, versioned reference that wrapper authors consult before writing a single line of wrapper code.

## Where to start

Read **[GUIDE.md](./GUIDE.md)**. It covers all six hard rules, rationale for each, and four reference examples (three safe patterns and one anti-pattern).

## Who should read this

Anyone building or reviewing Buff wrapper crates: Wave 4 wrappers (buff-web, buff-db, buff-template, buff-reactive, buff-observe), community-contributed bindings, and framework reviewers.

## Scope

These rules apply to every `extern` declaration in `.buff` source and every Rust wrapper function that sits behind such a declaration. They do not cover the compiler's internal codegen (that is the domain of `buff-lang-codegen-rust`), but the guide documents how the compiler currently handles unsafe wrapping so wrapper authors understand the full picture.
