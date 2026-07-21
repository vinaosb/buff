use std::fmt;

/// An opaque entity identifier, returned by [`World::spawn`] and
/// consumed by [`World::insert`] / [`World::remove`] / [`World::despawn`].
///
/// Internally this is a `(u32, u32)` id+generation pair wrapped in
/// `hecs::Entity`. Buff users never see the inner layout — they
/// compare entities by value, store them in collections, and pass them
/// back to the world. (Copy + Eq + Hash so `Entity` works as a flat
/// key without explicit derefs.)
///
/// # FFI safety
///
/// - **R1** (no raw pointers): `Entity` is a transparent newtype over
///   two `u32`s. No `*const T` / `*mut T` in the public surface.
/// - **R4** (Send + 'static): `Entity` is `Copy + Send + Sync + 'static`.
///   Safe to capture in spawn closures.
/// - **R5** (no lifetimes): `Entity` is owned and `'static`; no borrow
///   of the [`World`](crate::World) it came from.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Entity {
    pub(crate) inner: hecs::Entity,
}

impl Entity {
    /// The raw id of this entity (the `u32` slot in the world's
    /// entity slab). Stable for the entity's lifetime; never reused
    /// by a different live entity.
    pub fn id(&self) -> u64 {
        self.inner.id() as u64
    }

    /// Convert a `hecs::Entity` into the Buff-public wrapper. Used
    /// internally by [`World::spawn`](crate::World::spawn) so the
    /// public surface never exposes the hecs type.
    pub(crate) fn from_hecs(inner: hecs::Entity) -> Self {
        Self { inner }
    }

    /// Convert back to the inner `hecs::Entity`. Used internally by
    /// every [`World`](crate::World) method that needs to look the
    /// entity up in the backing store.
    pub(crate) fn as_hecs(&self) -> hecs::Entity {
        self.inner
    }
}

impl fmt::Debug for Entity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Entity")
            .field("id", &self.id())
            .finish_non_exhaustive()
    }
}

impl fmt::Display for Entity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Entity#{}", self.id())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entity_is_copy_eq_hash() {
        let mut world = hecs::World::new();
        let e = world.spawn(("a", 1_i32));
        let wrapped = Entity::from_hecs(e);
        let copied = wrapped;
        assert_eq!(wrapped, copied);
        let mut set = std::collections::HashSet::new();
        set.insert(wrapped);
        assert!(set.contains(&copied));
    }

    #[test]
    fn entity_debug_display_includes_id() {
        let mut world = hecs::World::new();
        let e = world.spawn(("x",));
        let wrapped = Entity::from_hecs(e);
        let s = format!("{:?}", wrapped);
        assert!(s.contains("id"));
        let display = format!("{}", wrapped);
        assert!(display.starts_with("Entity#"));
    }
}
