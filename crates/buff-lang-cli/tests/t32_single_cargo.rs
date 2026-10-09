//! T32/ITER-56B: single-file Cargo-project linking e2e - the first
//! framework program to compile AND execute through the workspace
//! (Tensor lowering from PR #139 + this slice's linker).

use std::process::Command;

const TENSOR_SRC: &str =
    "func main():\n    let t = Tensor.zeros([3, 4])\n    let dims = t.shape()\n    print(t.rank())\n    print(t.len())\n";

fn cargo_on_path() -> bool {
    Command::new("cargo").arg("--version").output().is_ok()
}

#[test]
fn tensor_program_links_via_cargo_and_runs() {
    if !cargo_on_path() {
        eprintln!("skipping: cargo not on PATH");
        return;
    }
    // The test binary may live OUTSIDE the workspace (shared
    // CARGO_TARGET_DIR builds, e.g. the local-ci gate's C:\ct) where
    // exe-relative discovery finds no workspace. Pin the compile-time
    // workspace root instead.
    // No canonicalize(): on Windows it returns \\?\ extended-length
    // paths, which cargo rejects as path-dep URLs.
    let ws_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("workspace root from CARGO_MANIFEST_DIR");
    std::env::set_var("BUFF_WORKSPACE_ROOT", ws_root);
    let dir = std::env::temp_dir().join(format!("buff-t32-e2e-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("mkdir");
    let buff = dir.join("tensor_hello.buff");
    std::fs::write(&buff, TENSOR_SRC).expect("write .buff fixture");

    let out = buff_lang_cli::pipeline::compile_to_rust(&buff).expect("compile_to_rust");
    assert!(
        out.extern_crates.contains("buff-tensor"),
        "extern set should contain buff-tensor: {:?}",
        out.extern_crates
    );

    let exe_name = if cfg!(windows) {
        "tensor_hello.exe"
    } else {
        "tensor_hello"
    };
    let exe = dir.join(exe_name);
    buff_lang_cli::project_pipeline::build_single_via_cargo(
        &out.rust_file_path,
        &out.extern_crates,
        "tensor_hello",
        &exe,
        buff_lang_cli::pipeline::BuildMode::Debug,
    )
    .expect("cargo link + build");

    let run = Command::new(&exe).output().expect("run exe");
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(
        run.status.success(),
        "exit {:?}; stderr: {}",
        run.status.code(),
        String::from_utf8_lossy(&run.stderr)
    );
    assert!(stdout.contains("2"), "rank line: {stdout}");
    assert!(stdout.contains("12"), "len line: {stdout}");
    let _ = std::fs::remove_dir_all(&dir);
}
