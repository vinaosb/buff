/// Error variants for [`World`](crate::World) operations that can fail
/// for more than one reason.
///
/// # FFI safety
///
/// Per Rule R3 of `crates/buff-lang-ffi-guide/GUIDE.md`, fallible
/// operations return `Result<T, EcsError>` so callers can match on the
/// variant. The error variants carry no spans (the FFI guide documents
/// this as the current convention until wrapper infrastructure threads
/// Buff spans through).
///
/// `Display` is derived via `thiserror::Error`'s `#[error("...")]`
/// attributes on each variant — do NOT add a manual `impl Display`
/// (conflicting implementations of `std::fmt::Display` for `EcsError`).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EcsError {
    /// `world.insert(entity, c)` / `world.remove::<T>(entity)` /
    /// `world.get_mut::<T>(entity)` was called with an [`Entity`](crate::Entity)
    /// that has been despawned (or never existed).
    #[error("entity not found: {0}")]
    EntityMissing(u64),

    /// `world.remove::<T>(entity)` / `world.get_mut::<T>(entity)` was
    /// called on a live entity that has no component of type `T`.
    /// Carries the type name for diagnostics.
    #[error("entity {entity_id} has no component of type {type_name}")]
    ComponentMissing {
        entity_id: u64,
        type_name: &'static str,
    },

    /// A registered [system](crate::System) returned an error from its
    /// `run` callback. Carries the system's diagnostic name and the
    /// reduced-to-`String` error payload.
    #[error("system `{system_name}` failed: {message}")]
    SystemFailed {
        system_name: String,
        message: String,
    },
}

impl EcsError {
    #[allow(dead_code)]
    pub(crate) fn entity_id(&self) -> Option<u64> {
        match self {
            EcsError::EntityMissing(id) => Some(*id),
            EcsError::ComponentMissing { entity_id, .. } => Some(*entity_id),
            EcsError::SystemFailed { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entity_missing_carries_id() {
        let e = EcsError::EntityMissing(42);
        assert_eq!(e.entity_id(), Some(42));
        let s = format!("{e}");
        assert!(s.contains("42"));
    }

    #[test]
    fn component_missing_carries_id_and_type() {
        let e = EcsError::ComponentMissing {
            entity_id: 7,
            type_name: "Velocity",
        };
        assert_eq!(e.entity_id(), Some(7));
        let s = format!("{e}");
        assert!(s.contains("7"));
        assert!(s.contains("Velocity"));
    }

    #[test]
    fn system_failed_carries_name_and_message() {
        let e = EcsError::SystemFailed {
            system_name: "physics".to_string(),
            message: "overflow".to_string(),
        };
        assert_eq!(e.entity_id(), None);
        let s = format!("{e}");
        assert!(s.contains("physics"));
        assert!(s.contains("overflow"));
    }
}
