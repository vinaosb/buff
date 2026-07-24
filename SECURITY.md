# Security Policy

## Supported Versions

| Version | Supported |
|---------|-----------|
| v1.x    | Yes       |
| v0.x    | No        |

## Reporting a Vulnerability

Buff transpiles to Rust and compiles via rustc, so it inherits Rust's memory safety guarantees for all generated code. Compiler bugs that could produce unsafe Rust output are treated as security issues.

If you find a security vulnerability, please report it privately:

- **Email**: vinaosb@gmail.com
- **Subject line**: `[SECURITY] Buff vulnerability report`

Please include:

1. A description of the vulnerability
2. Steps to reproduce it
3. The affected Buff version (`buff --version`)
4. Any potential impact you have identified

## What to Expect

- **Acknowledgment within 48 hours** that your report was received
- **An initial assessment within 7 days**
- **A fix released within 90 days** (or a clear timeline if the fix is more complex)

## Disclosure Policy

Do not publicly disclose the vulnerability until a fix has been released. We will coordinate with you on a disclosure timeline and credit you in the release notes (unless you prefer to remain anonymous).

## Scope

This policy covers:

- The Buff compiler (`buff-lang-*` crates)
- The runtime (`buff-lang-runtime`, GPU/WGSL codegen)
- The CLI, REPL, LSP server, and Jupyter kernel
- The package registry server

Third-party dependencies are tracked via Dependabot and addressed through normal update cycles.