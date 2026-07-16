//! Symbol table mapping variable names to their inferred [`Type`].

use std::collections::HashMap;

use crate::ty::Type;

/// A flat type environment: a map from variable name to its [`Type`].
///
/// v0.1 uses a single flat scope. Lexical scoping / shadowing semantics will
/// be layered on top in a later task.
#[derive(Debug, Clone, Default)]
pub struct TypeEnv {
    bindings: HashMap<String, Type>,
}

impl TypeEnv {
    /// Creates an empty type environment.
    pub fn new() -> Self {
        Self::default()
    }

    /// Binds (or re-binds) `name` to `ty`.
    pub fn insert(&mut self, name: &str, ty: Type) {
        self.bindings.insert(name.to_string(), ty);
    }

    /// Returns the type bound to `name`, if any.
    pub fn lookup(&self, name: &str) -> Option<&Type> {
        self.bindings.get(name)
    }

    /// Removes the binding for `name`, if present.
    pub fn remove(&mut self, name: &str) {
        self.bindings.remove(name);
    }

    /// Returns `true` if the environment contains no bindings.
    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_and_lookup() {
        let mut env = TypeEnv::new();
        assert!(env.is_empty());
        env.insert("x", Type::int_default());
        assert!(!env.is_empty());
        assert_eq!(env.lookup("x"), Some(&Type::int_default()));
        assert_eq!(env.lookup("missing"), None);
    }

    #[test]
    fn rebind_changes_type() {
        let mut env = TypeEnv::new();
        env.insert("x", Type::int_default());
        env.insert("x", Type::string());
        assert_eq!(env.lookup("x"), Some(&Type::string()));
    }

    #[test]
    fn remove_binding() {
        let mut env = TypeEnv::new();
        env.insert("x", Type::bool());
        env.remove("x");
        assert_eq!(env.lookup("x"), None);
    }
}
