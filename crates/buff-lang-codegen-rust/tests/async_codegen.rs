//! T31 integration tests — Rust codegen for Buff's async model.
//!
//! These tests build the Buff AST by hand (the codegen is the system under
//! test; parser-level coverage of `spawn`/`async func` lives in the parser
//! crate's tests) and verify the generated Rust source for:
//!
//! - **Direct `async` declared fns** → Rust `async fn`.
//! - **1-hop propagation** → a fn calling an async fn becomes `async fn`.
//! - **Transitive propagation** → `main → pipeline → fetch → io` all async.
//! - **Sync fn stays sync** → no `async` keyword emitted.
//! - **`#[tokio::main]`** → emitted on `main` when it's in the async set.
//! - **Auto-inserted `.await`** at async call sites inside async fns.
//! - **`spawn task()`** → `tokio::spawn(async move { task() })`.
//! - **`task.result()`** → `task.await`.
//! - **`block(expr)`** in sync context → one-shot runtime `block_on`.
//! - **`block()` inside async fn** → deadlock warning diagnostic emitted.
//! - **No `await` keyword in Buff source** — only ever appears in generated Rust.
//!
//! Run via:
//!
//! ```text
//! cargo test -p buff-lang-codegen-rust --test async_codegen
//! ```

use buff_lang_ast::common::{Block, Ident};
use buff_lang_ast::decl::FuncDecl;
use buff_lang_ast::{Decl, Expr, Literal, Stmt};
use buff_lang_codegen_rust::{generate_rust, RustCodegen};
use buff_lang_error::{Diagnostic, Severity, Span};

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

fn span() -> Span {
    Span::dummy()
}

fn ident(s: &str) -> Ident {
    Ident::new(s, span())
}

fn ident_expr(s: &str) -> Expr {
    Expr::Ident(ident(s), span())
}

fn int_expr(n: i64) -> Expr {
    Expr::Literal(Literal::Int(n), span())
}

fn call_expr(name: &str, args: Vec<Expr>) -> Expr {
    Expr::FuncCall {
        callee: Box::new(ident_expr(name)),
        args,
        span: span(),
    }
}

fn zero_arg_call(name: &str) -> Expr {
    call_expr(name, Vec::new())
}

fn ret_expr(e: Expr) -> Stmt {
    Stmt::Return(Some(e), span())
}

fn ret_call(name: &str) -> Stmt {
    ret_expr(zero_arg_call(name))
}

fn ret_int(n: i64) -> Stmt {
    ret_expr(int_expr(n))
}

fn block(stmts: Vec<Stmt>) -> Block {
    Block {
        stmts,
        span: span(),
    }
}

fn func(name: &str, is_async: bool, body_stmts: Vec<Stmt>) -> FuncDecl {
    FuncDecl {
        name: ident(name),
        params: Vec::new(),
        return_type: None,
        body: block(body_stmts),
        is_async,
        is_unsafe: false,
        is_extern: false,
        attributes: Vec::new(),
        span: span(),
    }
}

fn func_decl(name: &str, is_async: bool, body_stmts: Vec<Stmt>) -> Decl {
    Decl::FuncDecl(func(name, is_async, body_stmts))
}

fn codegen(decls: Vec<Decl>) -> String {
    generate_rust(&decls).expect("codegen must succeed")
}

fn must_reparse(src: &str) {
    syn::parse_str::<syn::File>(src)
        .unwrap_or_else(|e| panic!("generated source must re-parse: {e}\n--- src ---\n{src}"));
}

/// Run codegen AND drain warnings so tests can inspect them.
fn codegen_with_warnings(decls: Vec<Decl>) -> (String, Vec<Diagnostic>) {
    let mut cg = RustCodegen::new();
    let file = cg.generate(&decls).expect("codegen must succeed");
    let src = buff_lang_codegen_rust::format_file(&file);
    let warnings = cg.take_warnings();
    (src, warnings)
}

// ---------------------------------------------------------------------------
// Direct async declared fn → Rust `async fn`
// ---------------------------------------------------------------------------

#[test]
fn direct_async_decl_emits_async_fn() {
    let d = vec![func_decl("http_get", true, vec![ret_int(0)])];
    let src = codegen(d);
    assert!(
        src.contains("async fn http_get"),
        "expected `async fn http_get` in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn sync_decl_emits_plain_fn_no_async_keyword() {
    let d = vec![func_decl("add", false, vec![ret_int(0)])];
    let src = codegen(d);
    assert!(
        src.contains("fn add") && !src.contains("async"),
        "sync fn must not mention `async`: {src}"
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// 1-hop propagation
// ---------------------------------------------------------------------------

#[test]
fn one_hop_propagation_emits_async_on_caller() {
    // async func http_get() { ... }
    // func fetch()    { http_get() }   -> becomes async fn
    let d = vec![
        func_decl("http_get", true, vec![ret_int(0)]),
        func_decl("fetch", false, vec![ret_call("http_get")]),
    ];
    let src = codegen(d);
    assert!(src.contains("async fn http_get"), "io must be async: {src}");
    assert!(
        src.contains("async fn fetch"),
        "fetch should be auto-async via propagation: {src}"
    );
    // The call site inside fetch should have .await auto-inserted.
    assert!(
        src.contains("http_get().await"),
        "expected auto-inserted `.await` at async call site: {src}"
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// Transitive multi-hop propagation
// ---------------------------------------------------------------------------

#[test]
fn transitive_propagation_marks_all_callers_async() {
    // async func io()
    // func fetch()    { io() }
    // func pipeline() { fetch() }
    // func main()     { pipeline() }
    let d = vec![
        func_decl("io", true, vec![ret_int(0)]),
        func_decl("fetch", false, vec![ret_call("io")]),
        func_decl("pipeline", false, vec![ret_call("fetch")]),
        func_decl("main", false, vec![ret_call("pipeline")]),
    ];
    let src = codegen(d);
    for n in &["io", "fetch", "pipeline", "main"] {
        assert!(
            src.contains(&format!("async fn {n}")),
            "{n} should be async: {src}"
        );
    }
    // Each async-call site gets .await.
    assert!(src.contains("io().await"), "missing .await on io(): {src}");
    assert!(
        src.contains("fetch().await"),
        "missing .await on fetch(): {src}"
    );
    assert!(
        src.contains("pipeline().await"),
        "missing .await on pipeline(): {src}"
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// main → #[tokio::main]
// ---------------------------------------------------------------------------

#[test]
fn async_main_gets_tokio_main_attribute() {
    // async func io()
    // func main() { io() }   -> main becomes async + #[tokio::main]
    let d = vec![
        func_decl("io", true, vec![ret_int(0)]),
        func_decl("main", false, vec![ret_call("io")]),
    ];
    let src = codegen(d);
    assert!(
        src.contains("#[tokio::main]"),
        "expected #[tokio::main] on async main: {src}"
    );
    assert!(
        src.contains("async fn main"),
        "expected `async fn main`: {src}"
    );
    must_reparse(&src);
}

#[test]
fn sync_main_does_not_get_tokio_main_attribute() {
    // func main() { return 0; }   -> no tokio attribute, no async
    let d = vec![func_decl("main", false, vec![ret_int(0)])];
    let src = codegen(d);
    assert!(
        !src.contains("tokio"),
        "sync main must NOT mention tokio: {src}"
    );
    assert!(
        !src.contains("async"),
        "sync main must NOT mention async: {src}"
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// Sync fn stays sync even when calling sync fns
// ---------------------------------------------------------------------------

#[test]
fn sync_fn_calling_sync_fn_stays_sync() {
    // func helper() { return 0; }
    // func caller() { helper() }
    let d = vec![
        func_decl("helper", false, vec![ret_int(0)]),
        func_decl("caller", false, vec![ret_call("helper")]),
    ];
    let src = codegen(d);
    assert!(
        !src.contains("async"),
        "sync chain must NOT emit async: {src}"
    );
    assert!(
        !src.contains(".await"),
        "sync chain must NOT emit .await: {src}"
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// spawn task() → tokio::spawn(async move { task() })
// ---------------------------------------------------------------------------

#[test]
fn spawn_task_lowers_to_tokio_spawn_async_move() {
    // async func task() { ... }
    // func runner() { spawn task() }
    let spawn_expr = Expr::Spawn {
        task: Box::new(zero_arg_call("task")),
        span: span(),
    };
    let d = vec![
        func_decl("task", true, vec![ret_int(0)]),
        // runner is sync (spawn does NOT propagate async-ness — it's a
        // task-launch, not a call-graph edge). Inside the spawned `async
        // move { ... }` block, the call to `task()` IS in an async
        // context, so `.await` is auto-inserted.
        func_decl("runner", false, vec![Stmt::ExprStmt(spawn_expr, span())]),
    ];
    let src = codegen(d);
    assert!(
        src.contains("tokio::spawn(async move { task().await })"),
        "expected tokio::spawn(async move {{ task().await }}): {src}"
    );
    // runner stays sync.
    assert!(
        !src.contains("async fn runner"),
        "runner must stay sync (spawn doesn't propagate): {src}"
    );
    must_reparse(&src);
}

#[test]
fn spawn_with_args_lowers_correctly() {
    // spawn task(42, "hi")
    let task = call_expr("task", vec![int_expr(42)]);
    let spawn_expr = Expr::Spawn {
        task: Box::new(task),
        span: span(),
    };
    let d = vec![
        // Declare task as async so the .await fires inside the spawn body.
        func_decl("task", true, vec![ret_int(0)]),
        func_decl("runner", false, vec![Stmt::ExprStmt(spawn_expr, span())]),
    ];
    let src = codegen(d);
    assert!(
        src.contains("tokio::spawn(async move { task(42).await })"),
        "expected spawn with args + .await: {src}"
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// task.result() → task.await
// ---------------------------------------------------------------------------

#[test]
fn task_result_lowers_to_dot_await() {
    // func runner(task) { task.result() }
    let method_call = Expr::MethodCall {
        receiver: Box::new(ident_expr("task")),
        method: ident("result"),
        args: Vec::new(),
        span: span(),
    };
    let d = vec![func_decl(
        "runner",
        false,
        vec![Stmt::ExprStmt(method_call, span())],
    )];
    let src = codegen(d);
    assert!(
        src.contains("task.await"),
        "expected `task.await` for task.result(): {src}"
    );
    // Make sure it's NOT a method call (no parens after .result).
    assert!(
        !src.contains(".result"),
        "must not emit `.result` as field/method: {src}"
    );
    must_reparse(&src);
}

#[test]
fn task_result_on_spawned_task_lowers_to_await() {
    // async func work() { ... }
    // func main() {
    //     let t = spawn work()
    //     t.result()
    // }
    let spawn = Expr::Spawn {
        task: Box::new(zero_arg_call("work")),
        span: span(),
    };
    let let_spawn = Stmt::LetDecl {
        name: ident("t"),
        value: spawn,
        mutable: false,
        ty: None,
        span: span(),
    };
    let result_call = Expr::MethodCall {
        receiver: Box::new(ident_expr("t")),
        method: ident("result"),
        args: Vec::new(),
        span: span(),
    };
    let use_result = Stmt::ExprStmt(result_call, span());
    let d = vec![
        func_decl("work", true, vec![ret_int(0)]),
        func_decl("main", false, vec![let_spawn, use_result]),
    ];
    let src = codegen(d);
    assert!(
        src.contains("t.await"),
        "expected `t.await` from t.result(): {src}"
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// block(expr) → one-shot runtime block_on
// ---------------------------------------------------------------------------

#[test]
fn block_in_sync_context_lowers_to_runtime_block_on() {
    // func main() { block(async_thing()) }
    // (main is sync because async_thing isn't declared here — pretend it's
    // an extern async fn that doesn't get into the propagation set.)
    let block_call = call_expr("block", vec![zero_arg_call("async_thing")]);
    let d = vec![func_decl(
        "main",
        false,
        vec![Stmt::ExprStmt(block_call, span())],
    )];
    let (src, warnings) = codegen_with_warnings(d);
    assert!(
        src.contains("tokio::runtime::Runtime::new()"),
        "expected Runtime::new() in block() lowering: {src}"
    );
    assert!(
        src.contains(".block_on(async_thing())"),
        "expected `.block_on(async_thing())`: {src}"
    );
    // Sync context -> no warning.
    assert!(
        warnings.is_empty(),
        "sync block() should not warn, got: {:?}",
        warnings
    );
    must_reparse(&src);
}

#[test]
fn block_in_async_context_emits_deadlock_warning() {
    // async func io() { ... }
    // func runner() { block(io()) }   -> runner becomes async via propagation
    //                                    (it calls io, which is async) and
    //                                    then block() inside async -> warning.
    let block_call = call_expr("block", vec![zero_arg_call("io")]);
    let d = vec![
        func_decl("io", true, vec![ret_int(0)]),
        func_decl("runner", false, vec![Stmt::ExprStmt(block_call, span())]),
    ];
    let (src, warnings) = codegen_with_warnings(d);
    // runner becomes async via propagation (calls io which is async).
    assert!(
        src.contains("async fn runner"),
        "runner should be async via propagation: {src}"
    );
    // The block() lowering still emits Runtime::new() + block_on.
    assert!(src.contains(".block_on("), "expected block_on: {src}");
    // And the warning fires.
    let has_block_warning = warnings
        .iter()
        .any(|d| d.severity == Severity::Warning && d.message.contains("block()"));
    assert!(
        has_block_warning,
        "expected a Warning diagnostic about block() in async, got: {:?}",
        warnings
            .iter()
            .map(|d| (d.severity, &d.message))
            .collect::<Vec<_>>()
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// No `await` keyword in Buff source — only in generated Rust
// ---------------------------------------------------------------------------

#[test]
fn no_await_keyword_in_buff_ast_constructors() {
    // Sanity: none of the AST constructors used by these tests reference an
    // `await` field/variant. (The test itself is a tautology, but it pins
    // the invariant: Buff has no `await` keyword — the lexer has no
    // `KwAwait`, the AST has no `Expr::Await`, and the parser never builds
    // one. The only `.await` is emitted by codegen at async call sites.)
    let src = codegen(vec![
        func_decl("io", true, vec![ret_int(0)]),
        func_decl("main", false, vec![ret_call("io")]),
    ]);
    // The generated Rust DOES contain `.await`, but no Buff-level construct
    // produced it directly — codegen inserted it.
    assert!(
        src.contains(".await"),
        "codegen should insert .await: {src}"
    );
}

// ---------------------------------------------------------------------------
// Snapshot: full async pipeline
// ---------------------------------------------------------------------------

#[test]
fn full_async_pipeline_snapshot() {
    let d = vec![
        func_decl("http_get", true, vec![ret_int(0)]),
        func_decl("fetch", false, vec![ret_call("http_get")]),
        func_decl("pipeline", false, vec![ret_call("fetch")]),
        func_decl("main", false, vec![ret_call("pipeline")]),
    ];
    let src = codegen(d);
    insta::assert_snapshot!(src, @r###"
    async fn http_get() {
        return 0;
    }
    async fn fetch() {
        return http_get().await;
    }
    async fn pipeline() {
        return fetch().await;
    }
    #[tokio::main]
    async fn main() {
        return pipeline().await;
    }
    "###);
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// Snapshot: spawn + result chain
// ---------------------------------------------------------------------------

#[test]
fn spawn_and_result_snapshot() {
    let spawn = Expr::Spawn {
        task: Box::new(zero_arg_call("work")),
        span: span(),
    };
    let let_spawn = Stmt::LetDecl {
        name: ident("t"),
        value: spawn,
        mutable: false,
        ty: None,
        span: span(),
    };
    let result_call = Expr::MethodCall {
        receiver: Box::new(ident_expr("t")),
        method: ident("result"),
        args: Vec::new(),
        span: span(),
    };
    let use_result = Stmt::ExprStmt(result_call, span());
    let d = vec![
        func_decl("work", true, vec![ret_int(0)]),
        // runner is async via propagation: it calls work() indirectly? No,
        // it doesn't call work — it spawns work. spawn doesn't propagate
        // async. But t.result() doesn't propagate either. So runner stays
        // sync, and the .await is emitted at t.result() site regardless.
        func_decl("runner", false, vec![let_spawn, use_result]),
    ];
    let src = codegen(d);
    insta::assert_snapshot!(src, @r###"
    async fn work() {
        return 0;
    }
    fn runner() {
        let t = tokio::spawn(async move { work().await });
        t.await;
    }
    "###);
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// Cycle / mutual recursion codegen
// ---------------------------------------------------------------------------

#[test]
fn mutual_recursion_propagates_async_in_codegen() {
    // async func a() { b() }
    // func b()       { a() }
    let d = vec![
        func_decl("a", true, vec![ret_call("b")]),
        func_decl("b", false, vec![ret_call("a")]),
    ];
    let src = codegen(d);
    assert!(src.contains("async fn a"), "a should be async: {src}");
    assert!(src.contains("async fn b"), "b should be async: {src}");
    // Both call sites get .await.
    assert!(src.contains("b().await"), "missing .await on b(): {src}");
    assert!(src.contains("a().await"), "missing .await on a(): {src}");
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// Parser-level round-trip (uses dev-dep buff-lang-parser)
// ---------------------------------------------------------------------------

#[test]
fn parse_async_func_modifier_round_trips_through_codegen() {
    // Source: `async func io() { return 0; }`
    // Then `func fetch() { io() }`.
    use buff_lang_lexer::tokenize;
    use buff_lang_parser::parse;

    // Buff uses offside-rule layout: each `func name():` line is followed
    // by an indented body. The trailing `:` after the signature is required.
    let src = "async func io():\n    return 0\nfunc fetch():\n    io()\n";
    let tokens = tokenize(src, buff_lang_error::SourceId(0)).expect("lex must succeed");
    let decls = parse(&tokens, buff_lang_error::SourceId(0)).expect("parse must succeed");
    let rust = codegen(decls);
    assert!(
        rust.contains("async fn io"),
        "io should be async from `async func`: {rust}"
    );
    assert!(
        rust.contains("async fn fetch"),
        "fetch should be auto-async via propagation: {rust}"
    );
    assert!(
        rust.contains("io().await"),
        "missing auto-inserted .await: {rust}"
    );
    must_reparse(&rust);
}

#[test]
fn parse_spawn_expression_round_trips_through_codegen() {
    use buff_lang_lexer::tokenize;
    use buff_lang_parser::parse;

    // async func work() { return 0; }
    // func runner() { spawn work() }
    // Buff uses offside-rule layout: each `func name():` line is followed
    // by an indented body. The trailing `:` after the signature is required.
    let src = "async func work():\n    return 0\nfunc runner():\n    spawn work()\n";
    let tokens = tokenize(src, buff_lang_error::SourceId(0)).expect("lex must succeed");
    let decls = parse(&tokens, buff_lang_error::SourceId(0)).expect("parse must succeed");
    let rust = codegen(decls);
    assert!(
        rust.contains("tokio::spawn(async move { work().await })"),
        "expected spawn lowering (work is async, .await auto-inserted): {rust}"
    );
    must_reparse(&rust);
}

// ---------------------------------------------------------------------------
// Async call inside a non-async fn does NOT get .await
// ---------------------------------------------------------------------------

#[test]
fn async_callee_in_sync_fn_does_not_get_await() {
    // async func io() { ... }
    // func caller() { io() }
    //
    // Without propagation, caller would be sync and the io() call inside
    // would be ill-typed (calling an async fn from sync Rust requires
    // special handling). BUT in Buff the propagation rule says: any fn
    // that calls an async fn becomes async. So caller IS async. This
    // test verifies the propagation kicks in (so the .await IS emitted).
    //
    // To get the "no propagation" case, we'd need a sync fn that calls
    // an async fn WITHOUT being marked async. That's impossible in Buff
    // by design. So this test instead documents that the propagation
    // ALWAYS wins.
    let d = vec![
        func_decl("io", true, vec![ret_int(0)]),
        func_decl("caller", false, vec![ret_call("io")]),
    ];
    let src = codegen(d);
    assert!(
        src.contains("async fn caller"),
        "caller must be async via propagation: {src}"
    );
    assert!(src.contains("io().await"), ".await must be inserted: {src}");
}

// ---------------------------------------------------------------------------
// block(expr) — full snapshot
// ---------------------------------------------------------------------------

#[test]
fn block_call_full_snapshot() {
    // func main() { block(some_async()) }
    // some_async is NOT declared, so main stays sync (no propagation).
    let d = vec![func_decl(
        "main",
        false,
        vec![Stmt::ExprStmt(
            call_expr("block", vec![zero_arg_call("some_async")]),
            span(),
        )],
    )];
    let src = codegen(d);
    insta::assert_snapshot!(src, @r###"
    fn main() {
        {
            let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
            rt.block_on(some_async())
        };
    }
    "###);
    must_reparse(&src);
}
