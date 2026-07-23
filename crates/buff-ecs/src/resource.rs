use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::fmt;

/// A typed resource map — the side-channel state for ECS systems.
///
/// Resources are values that don't belong to any single entity (global
/// game state, asset handles, configuration, frame counters, ...).
/// Stored by [`TypeId`] so a system can request "the `GameState`
/// resource" by type without naming a key.
///
/// Exposed for advanced use via [`World::resources`](crate::World) —
/// day-to-day code uses the typed `World::insert_resource` /
/// `World::get_resource` / `World::get_resource_mut` /
/// `World::remove_resource` shortcuts instead.
#[derive(Default)]
pub(crate) struct Resources {
    map: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
}

impl Resources {
    /// Create an empty resource map.
    pub(crate) fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    /// Insert a resource value, replacing any prior value of the same
    /// type. Returns the prior value if one was present.
    pub(crate) fn insert<T>(&mut self, value: T) -> Option<T>
    where
        T: Send + Sync + 'static,
    {
        let key = TypeId::of::<T>();
        let old = self.map.insert(key, Box::new(value));
        old.and_then(|boxed| boxed.downcast::<T>().ok().map(|b| *b))
    }

    /// Look up the resource value of type `T`. Returns `None` if no
    /// value of that type has been inserted.
    pub(crate) fn get<T>(&self) -> Option<&T>
    where
        T: Send + Sync + 'static,
    {
        let key = TypeId::of::<T>();
        self.map
            .get(&key)
            .and_then(|boxed| boxed.downcast_ref::<T>())
    }

    /// Look up the resource value of type `T` mutably.
    pub(crate) fn get_mut<T>(&mut self) -> Option<&mut T>
    where
        T: Send + Sync + 'static,
    {
        let key = TypeId::of::<T>();
        self.map
            .get_mut(&key)
            .and_then(|boxed| boxed.downcast_mut::<T>())
    }

    /// Remove and return the resource value of type `T`. Returns
    /// `None` if no value of that type was present.
    pub(crate) fn remove<T>(&mut self) -> Option<T>
    where
        T: Send + Sync + 'static,
    {
        let key = TypeId::of::<T>();
        self.map
            .remove(&key)
            .and_then(|boxed| boxed.downcast::<T>().ok().map(|b| *b))
    }

    /// Returns `true` if a resource of type `T` is present.
    #[allow(dead_code)]
    pub(crate) fn contains<T>(&self) -> bool
    where
        T: Send + Sync + 'static,
    {
        self.map.contains_key(&TypeId::of::<T>())
    }

    /// Returns the number of distinct resource types currently stored.
    pub(crate) fn len(&self) -> usize {
        self.map.len()
    }

    /// Returns `true` if no resources are stored.
    #[allow(dead_code)]
    pub(crate) fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Remove all resources.
    pub(crate) fn clear(&mut self) {
        self.map.clear();
    }
}

impl fmt::Debug for Resources {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Resources")
            .field("len", &self.map.len())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq)]
    struct Score(u32);

    #[derive(Debug, PartialEq)]
    struct PlayerName(String);

    #[test]
    fn insert_get_remove_roundtrip() {
        let mut r = Resources::new();
        assert!(r.is_empty());
        r.insert(Score(10));
        assert_eq!(r.len(), 1);
        assert_eq!(r.get::<Score>(), Some(&Score(10)));
        assert!(r.contains::<Score>());
        assert!(r.get::<PlayerName>().is_none());

        let removed = r.remove::<Score>();
        assert_eq!(removed, Some(Score(10)));
        assert!(r.is_empty());
    }

    #[test]
    fn insert_replaces_prior_value() {
        let mut r = Resources::new();
        let old = r.insert(Score(1));
        assert!(old.is_none());
        let old = r.insert(Score(2));
        assert_eq!(old, Some(Score(1)));
        assert_eq!(r.get::<Score>(), Some(&Score(2)));
    }

    #[test]
    fn get_mut_allows_mutation() {
        let mut r = Resources::new();
        r.insert(Score(0));
        if let Some(s) = r.get_mut::<Score>() {
            s.0 += 5;
        }
        assert_eq!(r.get::<Score>(), Some(&Score(5)));
    }

    #[test]
    fn distinct_types_coexist() {
        let mut r = Resources::new();
        r.insert(Score(42));
        r.insert(PlayerName("Hero".to_string()));
        assert_eq!(r.len(), 2);
        assert_eq!(r.get::<Score>(), Some(&Score(42)));
        assert_eq!(r.get::<PlayerName>(), Some(&PlayerName("Hero".to_string())));
        r.clear();
        assert!(r.is_empty());
    }
}
