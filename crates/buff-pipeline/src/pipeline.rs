//! [`Pipeline<T>`] — the core linear-chain type.
//!
//! A `Pipeline<T>` carries a continuation closure (`spawner`) that,
//! when invoked with a [`Sender<T>`], spawns the entire upstream chain
//! (source + all stages) on the current tokio runtime and returns a
//! [`JoinHandle`] that completes when the upstream work finishes. Each
//! call to a builder method ([`Pipeline::stage`] / [`Pipeline::map`]
//! / etc.) consumes `self`, wraps the previous spawner in a new
//! channel + task, and returns a new `Pipeline<U>` whose spawner drives
//! the extended chain.
//!
//! # Why the continuation pattern
//!
//! Heterogeneous stage types (`T -> U`, `U -> V`, ...) cannot be stored
//! in a single `Vec<Stage>` without type erasure. The continuation
//! pattern threads the type information through the Rust type system:
//! `Pipeline<T>` knows its output type is `T`, and each builder method
//! fixes the relationship between the old and new type parameters.
//! Internally the spawner closure captures the previous spawner (already
//! type-erased to `Box<dyn FnOnce(Sender<Old>) -> JoinHandle>`) plus
//! the new stage's transform closure, so the whole chain is a single
//! boxed closure — no `dyn Any`, no runtime type checks.
//!
//! # Lifecycle
//!
//! 1. `Pipeline::new()` returns `Pipeline<()>` with a no-op spawner.
//! 2. `.source(data)` (or `Source::from_csv(...)`) replaces the no-op
//!    spawner with one that drains `data` into a `Sender<T>`.
//! 3. Each `.stage(name, fn)` / `.map(fn)` / etc. wraps the previous
//!    spawner: creates a new `Channel<T, U>` pair, calls the previous
//!    spawner with the input sender, and spawns a task that drains the
//!    input receiver through `fn` into the output sender.
//! 4. `.run()` builds a multi-thread tokio runtime, creates the final
//!    output channel, invokes the spawner, and drains the output
//!    receiver into a `Vec<T>`.

use buff_lang_runtime::{Channel, Sender};
use tokio::task::JoinHandle;

use crate::error::{PipelineError, PipelineResult};
use crate::DEFAULT_BUFFER;

/// Linear chain of stages wired by bounded `Channel<T>` MPSC queues.
///
/// The type parameter `T` is the **current output type** of the
/// pipeline. [`Pipeline::new`] returns `Pipeline<()>`; calling
/// [`Pipeline::source`] (or [`Source::from_csv`](crate::Source::from_csv))
/// fixes `T` to the source's element type. Each subsequent stage
/// builder consumes `self` and returns a new `Pipeline<U>` whose output
/// type is the stage's output.
///
/// # Concurrency model
///
/// Each stage runs as its own tokio task. Tasks are created lazily
/// inside [`Pipeline::run`] — the builder methods only thread closures
/// and channel endpoints, they do not spawn. This means building a
/// pipeline is cheap (no runtime, no threads) and running it is the
/// single point where concurrency begins.
///
/// # Determinism
///
/// Single-task stages ([`stage`](Pipeline::stage) / [`map`](Pipeline::map)
/// / [`filter`](Pipeline::filter) / [`batch`](Pipeline::batch) /
/// [`window`](Pipeline::window)) preserve input order because each
/// drains its input channel sequentially and sends to its output
/// channel in the same order. [`Pipeline::parallel`] does NOT preserve
/// order — workers race and the dispatcher round-robins without
/// tracking per-worker completion order.
pub struct Pipeline<T: Send + 'static> {
    /// Continuation: given a Sender<T>, spawn the whole upstream chain
    /// and return a JoinHandle that completes when upstream finishes.
    pub(crate) spawner: Box<dyn FnOnce(Sender<T>) -> JoinHandle<()> + Send + 'static>,
    /// Bounded channel capacity for the next inter-stage queue.
    pub(crate) buffer_size: usize,
    /// Stage names for Debug output (ordered source → sink).
    pub(crate) stage_names: Vec<String>,
}

// ---------------------------------------------------------------------------
// Pipeline<()> — the empty builder returned by Pipeline::new().
// ---------------------------------------------------------------------------

impl Pipeline<()> {
    /// Construct an empty untyped pipeline.
    ///
    /// Returns `Pipeline<()>` — call [`Pipeline::source`] (or use
    /// [`Source::from_csv`](crate::Source::from_csv)) next to fix the
    /// output type. The returned pipeline has a no-op spawner, so
    /// calling [`Pipeline::run`] on it directly returns `Ok(vec![])`.
    ///
    /// # Example
    ///
    /// ```rust
    /// use buff_pipeline::Pipeline;
    ///
    /// let p = Pipeline::new()
    ///     .source(vec![1, 2, 3, 4, 5])
    ///     .map(|x| x * 2)
    ///     .filter(|x| *x > 4);
    /// let result = p.run().expect("pipeline runs");
    /// assert_eq!(result, vec![6, 8, 10]);
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Pipeline {
            spawner: Box::new(|_sender: Sender<()>| tokio::spawn(async {})),
            buffer_size: DEFAULT_BUFFER,
            stage_names: Vec::new(),
        }
    }

    /// Fix the pipeline's output type by attaching an in-memory source.
    ///
    /// Consumes the `Pipeline<()>` builder and returns `Pipeline<T>`
    /// whose source drains `data` into the first inter-stage channel.
    /// Each item is sent one at a time; when `data` is exhausted the
    /// source's sender is dropped, signalling "no more items" to the
    /// downstream receiver.
    ///
    /// For file-backed sources, prefer [`Source::from_csv`](crate::Source::from_csv)
    /// which streams row-by-row with bounded memory.
    ///
    /// # Example
    ///
    /// ```rust
    /// use buff_pipeline::Pipeline;
    ///
    /// let p = Pipeline::new().source(vec!["hello".to_string(), "world".to_string()]);
    /// let out = p.run().expect("run");
    /// assert_eq!(out, vec!["hello".to_string(), "world".to_string()]);
    /// ```
    #[must_use]
    pub fn source<T: Send + 'static>(self, data: Vec<T>) -> Pipeline<T> {
        let buffer_size = self.buffer_size;
        let mut names = self.stage_names;
        names.push("source".to_string());
        Pipeline {
            spawner: Box::new(move |sender: Sender<T>| {
                tokio::spawn(async move {
                    for item in data {
                        if sender.send(item).await.is_err() {
                            // Downstream closed early (filter rejected
                            // everything, sink collect short-circuited,
                            // etc.). Stop pushing — the receiver is gone
                            // and further sends would only error again.
                            break;
                        }
                    }
                    // sender dropped here → downstream recv() returns None
                })
            }),
            buffer_size,
            stage_names: names,
        }
    }
}

impl Default for Pipeline<()> {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Pipeline<T> — typed builder methods available once a source is set.
// ---------------------------------------------------------------------------

impl<T: Send + 'static> Pipeline<T> {
    /// Override the bounded channel capacity for subsequent stages.
    ///
    /// The default is [`DEFAULT_BUFFER`] (64). Smaller values tighten
    /// backpressure (upstream blocks sooner); larger values amortize
    /// per-item wakeup overhead at the cost of higher peak memory.
    /// A value of `0` creates a rendezvous channel (send blocks until
    /// recv is ready) — valid but rarely what you want for a pipeline.
    ///
    /// Only affects channels created by stages added AFTER this call;
    /// previously-added stages keep their existing buffer size.
    #[must_use]
    pub fn with_buffer(mut self, n: usize) -> Self {
        self.buffer_size = n;
        self
    }

    /// Attach a generic named stage that transforms each item `T -> U`.
    ///
    /// The `name` is recorded in the pipeline's Debug output (useful
    /// for tracing / diagnostics); it does NOT affect runtime behavior.
    /// The transform closure must be `Fn(T) -> U + Send + Sync + 'static`
    /// (callable from the spawned task, which may run on any worker
    /// thread of the tokio multi-thread runtime).
    ///
    /// For the common cases prefer the stage helpers:
    /// [`map`](Pipeline::map) / [`filter`](Pipeline::filter) /
    /// [`batch`](Pipeline::batch) / [`window`](Pipeline::window) /
    /// [`parallel`](Pipeline::parallel).
    #[must_use]
    pub fn stage<U, F>(self, name: impl Into<String>, transform: F) -> Pipeline<U>
    where
        U: Send + 'static,
        F: Fn(T) -> U + Send + Sync + 'static,
    {
        let buffer_size = self.buffer_size;
        let prev_spawner = self.spawner;
        let mut names = self.stage_names;
        names.push(name.into());
        Pipeline {
            spawner: Box::new(move |out_sender: Sender<U>| {
                let (in_sender, mut receiver) = Channel::new::<T>(buffer_size);
                // Drive the previous chain — it will feed in_sender.
                let _prev_handle = prev_spawner(in_sender);
                tokio::spawn(async move {
                    while let Some(item) = receiver.recv().await {
                        let out = transform(item);
                        if out_sender.send(out).await.is_err() {
                            // Downstream closed; stop processing.
                            break;
                        }
                    }
                    // out_sender dropped here → signals downstream
                })
            }),
            buffer_size,
            stage_names: names,
        }
    }

    /// Attach a side-effecting sink that observes each item and passes it through.
    ///
    /// The `side_effect` closure is called by reference (`Fn(&T)`) for
    /// each item; the item is then forwarded to the output channel
    /// unchanged. Useful for logging, metrics, or debugging without
    /// disrupting the pipeline flow.
    ///
    /// This is NOT terminal — the pipeline continues after the sink.
    /// Use [`Pipeline::run`] (or [`Sink::collect`](crate::Sink::collect))
    /// to terminate and collect the output `Vec<T>`.
    #[must_use]
    pub fn sink<F>(self, side_effect: F) -> Pipeline<T>
    where
        F: Fn(&T) + Send + Sync + 'static,
    {
        let buffer_size = self.buffer_size;
        let prev_spawner = self.spawner;
        let mut names = self.stage_names;
        names.push("sink".to_string());
        Pipeline {
            spawner: Box::new(move |out_sender: Sender<T>| {
                let (in_sender, mut receiver) = Channel::new::<T>(buffer_size);
                let _prev_handle = prev_spawner(in_sender);
                tokio::spawn(async move {
                    while let Some(item) = receiver.recv().await {
                        side_effect(&item);
                        if out_sender.send(item).await.is_err() {
                            break;
                        }
                    }
                })
            }),
            buffer_size,
            stage_names: names,
        }
    }

    /// Run the pipeline to completion and collect the output into a `Vec<T>`.
    ///
    /// Builds a fresh multi-thread tokio runtime (one runtime per
    /// `run()` call — runtimes are NOT cached across calls), spawns
    /// every stage as a task, and drains the final output channel until
    /// the last sender drops (which signals "pipeline complete").
    ///
    /// # Errors
    ///
    /// Returns [`PipelineError::Runtime`] only if the tokio runtime
    /// itself fails to build (extremely unlikely on a healthy host).
    ///
    /// # Blocking
    ///
    /// `run()` blocks the calling thread until the pipeline completes.
    /// Internally it enters `Runtime::block_on`, which parks the
    /// calling thread while async tasks run on worker threads. For
    /// non-blocking consumption, wrap the pipeline in your own tokio
    /// runtime and drive the spawner manually (advanced; not exposed
    /// by the MVP surface).
    ///
    /// # Example
    ///
    /// ```rust
    /// use buff_pipeline::Pipeline;
    ///
    /// let out = Pipeline::new()
    ///     .source(vec![1, 2, 3])
    ///     .map(|x| x + 10)
    ///     .run()
    ///     .expect("run");
    /// assert_eq!(out, vec![11, 12, 13]);
    /// ```
    pub fn run(self) -> PipelineResult<Vec<T>> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|e| PipelineError::Runtime {
                detail: e.to_string(),
            })?;

        runtime.block_on(async move {
            let (sender, mut receiver) = Channel::new::<T>(self.buffer_size);
            // Spawn the whole chain — every upstream stage task starts
            // running concurrently on the runtime's worker pool.
            let _top_handle = (self.spawner)(sender);
            let mut out = Vec::new();
            while let Some(item) = receiver.recv().await {
                out.push(item);
            }
            // receiver returned None → all senders dropped → every
            // upstream task has finished its send loop.
            Ok(out)
        })
    }
}

impl<T: Send + 'static> std::fmt::Debug for Pipeline<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pipeline")
            .field("stages", &self.stage_names)
            .field("buffer_size", &self.buffer_size)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_pipeline_returns_empty_vec() {
        let out: Vec<()> = Pipeline::new().run().expect("empty run");
        assert!(out.is_empty());
    }

    #[test]
    fn debug_shows_stage_names() {
        let p = Pipeline::new()
            .source(vec![1, 2, 3])
            .map(|x| x * 2)
            .filter(|x| *x > 2);
        let debug = format!("{:?}", p);
        assert!(debug.contains("source"));
        assert!(debug.contains("map"));
        assert!(debug.contains("filter"));
    }
}
