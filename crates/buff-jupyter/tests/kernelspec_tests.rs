//! Integration tests for the kernelspec generation + writer — verifies
//! the exact JSON shape Jupyter expects and the install-path
//! resolution rules.

use buff_jupyter::kernelspec::{
    buff_kernelspec_dir, jupyter_kernels_dir, write_kernel_json, KernelSpec, KERNEL_DISPLAY_NAME,
    KERNEL_LANGUAGE, KERNEL_NAME,
};
use serde_json::Value;
use std::path::PathBuf;

#[test]
fn kernel_spec_argv_invokes_buff_jupyter_start() {
    let spec = KernelSpec::buff("/usr/local/bin/buff");
    assert_eq!(
        spec.argv,
        vec![
            "/usr/local/bin/buff",
            "jupyter",
            "start",
            "--connection-file",
            "{connection_file}",
        ]
    );
}

#[test]
fn kernel_spec_advertises_canonical_names() {
    let spec = KernelSpec::buff("/bin/buff");
    assert_eq!(spec.display_name, KERNEL_DISPLAY_NAME);
    assert_eq!(spec.language, KERNEL_LANGUAGE);
}

#[test]
fn kernel_spec_uses_signal_interrupt_mode() {
    let spec = KernelSpec::buff("/bin/buff");
    // The kernel must declare its interrupt mode so Jupyter knows
    // whether to send SIGINT or an `interrupt_request` message.
    assert_eq!(spec.interrupt_mode, "signal");
}

#[test]
fn kernel_spec_serializes_to_pretty_json_with_required_fields() {
    let spec = KernelSpec::buff("/path/to/buff");
    let json = spec.to_json_pretty().expect("serialize");

    // Pretty JSON has newlines + indentation (2-space, serde default).
    assert!(json.contains('\n'));

    let v: Value = serde_json::from_str(&json).expect("re-parse");
    let obj = v.as_object().expect("object");
    for key in [
        "argv",
        "display_name",
        "language",
        "interrupt_mode",
        "metadata",
    ] {
        assert!(
            obj.contains_key(key),
            "kernel.json missing required field {key}"
        );
    }
    let argv = obj["argv"].as_array().expect("argv");
    assert_eq!(argv.len(), 5);
    assert_eq!(argv[4].as_str(), Some("{connection_file}"));
    assert_eq!(obj["display_name"].as_str(), Some(KERNEL_DISPLAY_NAME));
    assert_eq!(obj["language"].as_str(), Some(KERNEL_LANGUAGE));
}

#[test]
fn kernel_spec_round_trips_through_serde() {
    let spec = KernelSpec::buff("/bin/buff");
    let json = spec.to_json_pretty().expect("serialize");
    let parsed: KernelSpec = serde_json::from_str(&json).expect("parse");
    assert_eq!(parsed, spec);
}

#[test]
fn write_kernel_json_creates_file_on_disk() {
    let unique = format!(
        "buff-jupyter-int-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default()
    );
    let tmp_dir = std::env::temp_dir().join(unique);
    let dest = write_kernel_json(&KernelSpec::buff("/bin/buff"), &tmp_dir).expect("write");

    assert!(dest.ends_with("kernel.json"));
    assert!(dest.exists());

    let on_disk = std::fs::read_to_string(&dest).expect("read");
    let parsed: Value = serde_json::from_str(&on_disk).expect("parse json");
    assert_eq!(parsed["display_name"], KERNEL_DISPLAY_NAME);
    assert_eq!(parsed["language"], KERNEL_LANGUAGE);

    let _ = std::fs::remove_dir_all(&tmp_dir);
}

#[test]
fn jupyter_kernels_dir_resolves_on_normal_host() {
    // The kernels dir SHOULD resolve on any developer host (HOME or
    // APPDATA is always set). If this fails, the host environment is
    // broken in a way that would block `buff jupyter install` too.
    let dir: Option<PathBuf> = jupyter_kernels_dir();
    assert!(
        dir.is_some(),
        "jupyter_kernels_dir must resolve on a normal host"
    );
}

#[test]
fn buff_kernelspec_dir_appends_kernel_name() {
    if let Some(dir) = buff_kernelspec_dir() {
        let last = dir.file_name().and_then(|s| s.to_str()).unwrap_or_default();
        assert_eq!(last, KERNEL_NAME);
    }
}

#[test]
fn kernel_spec_argv_supports_paths_with_spaces() {
    // Windows install paths frequently contain spaces (Program Files).
    // The argv must serialize the full path verbatim, NOT split on
    // whitespace (Jupyter handles argv as exec(2) does — no shell).
    let spec = KernelSpec::buff("C:\\Program Files\\Buff\\buff.exe");
    assert_eq!(spec.argv[0], "C:\\Program Files\\Buff\\buff.exe");
}
