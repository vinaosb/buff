use crate::world::World;
use std::fmt;

/// A sequential pipeline step invoked by [`World::tick`](World::tick).
///
/// Systems are the only way to mutate entity/component state en masse
/// in [`World`]. Each system receives a mutable borrow of the world
/// and reads/writes components via the world's `for_each_*` /
/// `query_*` / `get_*` / `insert` / `remove` methods.
///
/// Register a system with [`World::add_system`](World::add_system) and
/// drive the pipeline with [`World::tick`](World::tick).
///
/// # Implementing a system
///
/// Most systems are short closures; [`SystemFn::new`] adapts any
/// `FnMut(&mut World)` closure into a [`System`]:
///
/// ```no_run
/// use buff_ecs::{World, SystemFn};
///
/// #[derive(Clone, Debug)]
/// struct Velocity { dx: f32, dy: f32 }
/// #[derive(Clone, Debug)]
/// struct Position { x: f32, y: f32 }
///
/// let mut world = World::new();
/// world.add_system(SystemFn::new(
///     "move".to_string(),
///     |w: &mut World| {
///         w.for_each_pair_mut(|_e, p: &mut Position, v: &mut Velocity| {
///             p.x += v.dx;
///             p.y += v.dy;
///         });
///     },
/// ));
/// world.tick();
/// ```
pub trait System: Send {
    /// Run the system's logic against the world.
    fn run(&mut self, world: &mut World);

    /// Diagnostic name — surfaced in [`EcsError::SystemFailed`](crate::EcsError::SystemFailed)
    /// if the system panics. Should be short, lowercase, kebab-case
    /// (e.g. `"move"`, `"physics"`, `"ai"`).
    fn name(&self) -> &str;
}

/// A [`System`] backed by a single `FnMut(&mut World)` closure.
///
/// Construct with [`SystemFn::new`].
pub struct SystemFn<F>
where
    F: FnMut(&mut World) + Send,
{
    system_name: String,
    callback: F,
}

impl<F> SystemFn<F>
where
    F: FnMut(&mut World) + Send,
{
    /// Wrap a closure as a [`System`]. The `system_name` is the
    /// diagnostic identifier surfaced in error messages if the
    /// closure panics inside [`World::tick`](World::tick).
    pub fn new(system_name: String, callback: F) -> Self {
        Self {
            system_name,
            callback,
        }
    }
}

impl<F> System for SystemFn<F>
where
    F: FnMut(&mut World) + Send,
{
    fn run(&mut self, world: &mut World) {
        (self.callback)(world);
    }

    fn name(&self) -> &str {
        &self.system_name
    }
}

impl<F> fmt::Debug for SystemFn<F>
where
    F: FnMut(&mut World) + Send,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SystemFn")
            .field("name", &self.system_name)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_fn_runs_callback() {
        use std::sync::{Arc, Mutex};
        let counter = Arc::new(Mutex::new(0_i32));
        let captured = Arc::clone(&counter);
        let mut s = SystemFn::new("inc".to_string(), move |_w: &mut World| {
            *captured.lock().unwrap() += 1;
        });
        let mut world = World::new();
        assert_eq!(s.name(), "inc");
        s.run(&mut world);
        s.run(&mut world);
        assert_eq!(*counter.lock().unwrap(), 2);
    }

    #[test]
    fn system_fn_name_persists() {
        let s = SystemFn::new("physics".to_string(), |_w: &mut World| {});
        assert_eq!(s.name(), "physics");
    }

    #[test]
    fn debug_format_includes_name() {
        let s = SystemFn::new("ai".to_string(), |_w: &mut World| {});
        let formatted = format!("{s:?}");
        assert!(formatted.contains("ai"));
        assert!(formatted.contains("SystemFn"));
    }
}
