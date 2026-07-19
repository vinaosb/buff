//! The compute-shader template — wraps a lowered body fragment in a complete
//! WGSL `@compute` shader with stable storage-buffer bindings.
//!
//! # Stable binding layout (DO NOT CHANGE without coordinating T45)
//!
//! The runtime crate (T45) hardcodes a [`wgpu::BindGroupLayout`] that matches
//! this exact layout. T44 owns the source-of-truth layout.
//!
//! | Binding                    | Usage                            |
//! |----------------------------|----------------------------------|
//! | `@group(0) @binding(0)`    | `var<storage, read> input`       |
//! | `@group(0) @binding(1)`    | `var<storage, read_write> output`|
//! | workgroup size             | `64` (X dimension)               |
//! | entry point                | `fn main(@builtin(global_invocation_id) gid: vec3<u32>)` |
//!
//! The shader unconditionally:
//! 1. Reads the X component of `global_invocation_id` as the element index.
//! 2. Bounds-checks against `arrayLength(&input)` and early-returns.
//! 3. Loads the element into a `let x = input[i];` binding matching the
//!    lambda parameter name.
//! 4. Lowers the body to a single WGSL expression.
//! 5. Writes the result to `output[i]`.
//!
//! [`wgpu::BindGroupLayout`]: https://docs.rs/wgpu/latest/wgpu/struct.BindGroupLayout.html

use crate::error::WgslError;
use crate::ty::WgslScalarType;

/// Options controlling the generated compute shader.
///
/// Defaults match the QA spec: `workgroup_size=64`, `element_type=f32`,
/// bindings `0`/`1` on group `0`. These are `#[derive(Clone, Copy)]` so they
/// flow trivially through `WgslCodegen::with_options(...)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WgslOptions {
    /// The X-dimension workgroup size. Default `64` (a common GPU warp-amplifier
    /// choice — 2 warps of 32 threads on NVIDIA, 4 wavefronts of 16 on AMD).
    pub workgroup_size: u32,
    /// The WGSL scalar type for the `input`/`output` storage buffers.
    pub element_type: WgslScalarType,
    /// The `@group(...)` index for both storage bindings. Default `0`.
    pub group: u32,
    /// The `@binding(...)` index for the `input` (read-only) storage buffer.
    pub binding_input: u32,
    /// The `@binding(...)` index for the `output` (read-write) storage buffer.
    pub binding_output: u32,
    /// Whether to emit a `let <param_name> = input[i];` binding inside the
    /// shader so the body can reference the parameter by name. Default `true`.
    /// If `false`, the body fragment is responsible for referencing
    /// `input[i]` directly (this is rarely what callers want).
    pub emit_param_binding: bool,
}

impl Default for WgslOptions {
    fn default() -> Self {
        Self {
            workgroup_size: 64,
            element_type: WgslScalarType::F32,
            group: 0,
            binding_input: 0,
            binding_output: 1,
            emit_param_binding: true,
        }
    }
}

impl WgslOptions {
    /// Validate the options, returning the first structural error if any.
    ///
    /// Currently checks:
    /// - `workgroup_size` must be in `1..=1024` (WGSL spec hard limit on a
    ///   single workgroup dimension).
    /// - `binding_input != binding_output` (otherwise the buffers alias).
    pub fn validate(&self) -> Result<(), WgslError> {
        if self.workgroup_size == 0 || self.workgroup_size > 1024 {
            return Err(WgslError::UnsupportedExpr {
                detail: format!(
                    "workgroup_size {} is out of range (must be 1..=1024 per WGSL spec)",
                    self.workgroup_size
                ),
            });
        }
        if self.binding_input == self.binding_output {
            return Err(WgslError::UnsupportedExpr {
                detail: format!(
                    "binding_input and binding_output must differ (both were {})",
                    self.binding_input
                ),
            });
        }
        Ok(())
    }
}

/// Render the complete WGSL compute shader source.
///
/// Inputs:
/// - `opts`: shader template options (bindings, workgroup size, element type).
/// - `param_name`: the name of the lambda parameter (e.g. `"x"`).
/// - `body_wgsl`: the already-lowered body fragment (e.g. `"x * 2.0"`).
///
/// Returns a complete, deterministic, byte-stable WGSL source string.
///
/// # Determinism
/// The output is a pure function of `(opts, param_name, body_wgsl)`. No
/// HashMap, no unstable iteration. The header comment is fixed-width and
/// does NOT embed timestamps or paths.
///
/// # Errors
/// Returns [`WgslError::UnsupportedExpr`] if `opts.validate()` fails.
pub fn render_shader(
    opts: &WgslOptions,
    param_name: &str,
    body_wgsl: &str,
) -> Result<String, WgslError> {
    opts.validate()?;

    let elem = opts.element_type.as_wgsl_keyword();
    let elem_array = opts.element_type.as_wgsl_array();
    let ws = opts.workgroup_size;
    let grp = opts.group;
    let bi = opts.binding_input;
    let bo = opts.binding_output;

    let mut out = String::with_capacity(1024);
    // Fixed-width deterministic header. The width is intentionally NOT
    // aligned — every shader starts with these exact 4 lines, byte-identical.
    out.push_str("// Auto-generated by buff-lang-codegen-wgsl. DO NOT EDIT.\n");
    out.push_str("// Map kernel body lowered from a Buff `{ param => <expr> }` lambda.\n");
    out.push_str("//\n");
    out.push_str(&format!(
        "// Element type: {elem} (Rust: {})\n",
        opts.element_type.as_rust_keyword()
    ));
    out.push_str(&format!("// Workgroup size: {ws}\n"));
    out.push_str(&format!("// Bindings: @group({grp}) @binding({bi})=input(read), @binding({bo})=output(read_write)\n"));
    out.push('\n');

    // Storage buffer declarations — one read-only input, one read_write output.
    out.push_str(&format!(
        "@group({grp}) @binding({bi}) var<storage, read> input: {elem_array};\n"
    ));
    out.push_str(&format!(
        "@group({grp}) @binding({bo}) var<storage, read_write> output: {elem_array};\n"
    ));
    out.push('\n');

    // Entry point.
    out.push_str(&format!("@compute @workgroup_size({ws})\n"));
    out.push_str("fn main(@builtin(global_invocation_id) gid: vec3<u32>) {\n");
    out.push_str("    let i = gid.x;\n");
    out.push_str("    if (i >= arrayLength(&input)) {\n");
    out.push_str("        return;\n");
    out.push_str("    }\n");
    if opts.emit_param_binding {
        out.push_str(&format!("    let {param_name} = input[i];\n"));
        out.push_str(&format!("    output[i] = {body_wgsl};\n"));
    } else {
        // No param binding: the body must reference `input[i]` itself. We
        // still write to output[i].
        out.push_str(&format!("    output[i] = {body_wgsl};\n"));
    }
    out.push_str("}\n");

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_opts() -> WgslOptions {
        WgslOptions::default()
    }

    #[test]
    fn validate_accepts_defaults() {
        assert!(default_opts().validate().is_ok());
    }

    #[test]
    fn validate_rejects_zero_workgroup_size() {
        let mut opts = default_opts();
        opts.workgroup_size = 0;
        assert!(opts.validate().is_err());
    }

    #[test]
    fn validate_rejects_too_large_workgroup_size() {
        let mut opts = default_opts();
        opts.workgroup_size = 2048;
        assert!(opts.validate().is_err());
    }

    #[test]
    fn validate_rejects_aliased_bindings() {
        let mut opts = default_opts();
        opts.binding_input = 5;
        opts.binding_output = 5;
        assert!(opts.validate().is_err());
    }

    #[test]
    fn render_shader_qa_case() {
        // QA case: {x => x * 2.0}
        let src = render_shader(&default_opts(), "x", "x * 2.0").unwrap();
        assert!(src.contains("@compute @workgroup_size(64)"));
        assert!(src.contains("@group(0) @binding(0) var<storage, read> input: array<f32>;"));
        assert!(src.contains("@group(0) @binding(1) var<storage, read_write> output: array<f32>;"));
        assert!(src.contains("let i = gid.x;"));
        assert!(src.contains("if (i >= arrayLength(&input))"));
        assert!(src.contains("let x = input[i];"));
        assert!(src.contains("output[i] = x * 2.0;"));
    }

    #[test]
    fn render_shader_deterministic_byte_identical() {
        let a = render_shader(&default_opts(), "x", "x * 2.0").unwrap();
        let b = render_shader(&default_opts(), "x", "x * 2.0").unwrap();
        assert_eq!(a, b, "same options → byte-identical shader source");
    }

    #[test]
    fn render_shader_respects_options() {
        let opts = WgslOptions {
            workgroup_size: 32,
            element_type: WgslScalarType::I32,
            group: 1,
            binding_input: 2,
            binding_output: 3,
            emit_param_binding: false,
        };
        let src = render_shader(&opts, "y", "y + 1").unwrap();
        assert!(src.contains("@compute @workgroup_size(32)"));
        assert!(src.contains("@group(1) @binding(2) var<storage, read> input: array<i32>;"));
        assert!(src.contains("@group(1) @binding(3) var<storage, read_write> output: array<i32>;"));
        // emit_param_binding=false → no `let y = input[i];` line
        assert!(!src.contains("let y = input[i];"));
        assert!(src.contains("output[i] = y + 1;"));
    }
}
