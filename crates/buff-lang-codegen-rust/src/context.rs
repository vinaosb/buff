//! Codegen context — tracks generated names, source-map info, and module structure.
//!
//! [`CodegenContext`] is owned by [`crate::RustCodegen`] during a single
//! code-generation pass. It accumulates:
//!
//! - [`source_mappings`]: maps Buff source [`Span`]s to `(line, col)` positions
//!   in the generated Rust output. Populated during codegen and consumed later
//!   by the source-map pass (T16).
//! - a counter for unique temporary-name generation.
//! - a `module_path` stack (for future use when nested modules are emitted).
//!
//! [`source_mappings`]: CodegenContext::source_mappings

use std::collections::HashMap;

use buff_lang_error::Span;

/// Per-pass state for Rust codegen.
///
/// Not shared across concurrent codegen runs — each [`crate::RustCodegen`]
/// owns its own. The state is small and cheap to clone if needed.
#[derive(Debug, Clone, Default)]
pub struct CodegenContext {
    /// Maps an AST [`Span`] (Buff byte offset) to a `(line, col)` pair in
    /// the generated Rust source. Populated during codegen for the source
    /// map; T16 will consume this.
    pub source_mappings: HashMap<Span, (usize, usize)>,

    /// Counter for generating unique temporary names.
    tmp_counter: u32,

    /// Module structure stack (for future use when nested modules emit).
    pub module_path: Vec<String>,
}

impl CodegenContext {
    /// Create an empty context.
    pub fn new() -> Self {
        Self::default()
    }

    /// Generate a fresh unique temporary variable identifier.
    ///
    /// Names look like `__buff_tmp_0`, `__buff_tmp_1`, … so they cannot
    /// collide with any user identifier (which never contains `__`).
    pub fn gen_tmp(&mut self) -> syn::Ident {
        let name = format!("__buff_tmp_{}", self.tmp_counter);
        self.tmp_counter += 1;
        syn::Ident::new(&name, proc_macro2::Span::call_site())
    }

    /// Record a Buff → Rust position mapping for the source map.
    pub fn record_mapping(&mut self, buff_span: Span, rust_line: usize, rust_col: usize) {
        self.source_mappings
            .insert(buff_span, (rust_line, rust_col));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gen_tmp_increments_counter() {
        let mut ctx = CodegenContext::new();
        let a = ctx.gen_tmp();
        let b = ctx.gen_tmp();
        assert_eq!(a.to_string(), "__buff_tmp_0");
        assert_eq!(b.to_string(), "__buff_tmp_1");
    }

    #[test]
    fn record_mapping_stores_position() {
        let mut ctx = CodegenContext::new();
        let span = Span::new(10, 20, buff_lang_error::SourceId(1));
        ctx.record_mapping(span, 3, 7);
        assert_eq!(ctx.source_mappings.get(&span), Some(&(3, 7)));
    }
}
