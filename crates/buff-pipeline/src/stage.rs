//! Stage helpers: common pipeline transformations expressed as one-call
//! builders on [`Pipeline<T>`](crate::Pipeline).
//!
//! Each helper is sugar over [`Pipeline::stage`](crate::Pipeline::stage)
//! (or a small variation for stages that need richer state, like
//! [`Pipeline::batch`] and [`Pipeline::parallel`]). They exist so users
//! can write `.map(|x| x * 2)` instead of `.stage("map", |x| x * 2)`.
//!
//! # Ordering
//!
//! [`Pipeline::map`] / [`Pipeline::filter`] / [`Pipeline::batch`] /
//! [`Pipeline::window`] preserve input order (single-task stage).
//! [`Pipeline::parallel`] does NOT preserve order — workers race and
//! the dispatcher round-robins without tracking per-worker completion.

use buff_lang_runtime::{Channel, Sender};
use tokio::task::JoinHandle;

use crate::Pipeline;

impl<T: Send + 'static> Pipeline<T> {
    /// Apply `f` to every item, preserving input order.
    ///
    /// Sugar for [`Pipeline::stage`] with the name `"map"`. The
    /// transform closure consumes the item (`Fn(T) -> U`, not `Fn(&T)
    /// -> U`); pass references inside the closure if you need to keep
    /// the source data.
    ///
    /// # Example
    ///
    /// ```rust
    /// use buff_pipeline::Pipeline;
    ///
    /// let out = Pipeline::new()
    ///     .source(vec![1, 2, 3])
    ///     .map(|x| x * 10)
    ///     .run()
    ///     .expect("run");
    /// assert_eq!(out, vec![10, 20, 30]);
    /// ```
    #[must_use]
    pub fn map<U, F>(self, f: F) -> Pipeline<U>
    where
        U: Send + 'static,
        F: Fn(T) -> U + Send + Sync + 'static,
    {
        self.stage("map", f)
    }

    /// Keep only items for which `pred` returns `true`, preserving
    /// input order.
    ///
    /// The predicate borrows each item (`Fn(&T) -> bool`) so `T` does
    /// not need to be `Clone` — kept items are forwarded in their
    /// original owned form.
    ///
    /// # Example
    ///
    /// ```rust
    /// use buff_pipeline::Pipeline;
    ///
    /// let out = Pipeline::new()
    ///     .source(vec![1, 2, 3, 4, 5])
    ///     .filter(|x| *x % 2 == 0)
    ///     .run()
    ///     .expect("run");
    /// assert_eq!(out, vec![2, 4]);
    /// ```
    #[must_use]
    pub fn filter<P>(self, pred: P) -> Pipeline<T>
    where
        P: Fn(&T) -> bool + Send + Sync + 'static,
    {
        let buffer_size = self.buffer_size;
        let prev_spawner = self.spawner;
        let mut names = self.stage_names;
        names.push("filter".to_string());
        Pipeline {
            spawner: Box::new(move |out_sender: Sender<T>| {
                let (in_sender, mut receiver) = Channel::new::<T>(buffer_size);
                let _prev_handle = prev_spawner(in_sender);
                tokio::spawn(async move {
                    while let Some(item) = receiver.recv().await {
                        if pred(&item) && out_sender.send(item).await.is_err() {
                            break;
                        }
                    }
                })
            }),
            buffer_size,
            stage_names: names,
        }
    }

    /// Group every `size` items into a `Vec<T>` and emit the batch.
    ///
    /// The final partial batch (if the total item count is not a
    /// multiple of `size`) is emitted as-is — callers that need
    /// exact-size batches should pad the source upstream.
    ///
    /// Output order is preserved (single-task stage).
    ///
    /// # Example
    ///
    /// ```rust
    /// use buff_pipeline::Pipeline;
    ///
    /// let out = Pipeline::new()
    ///     .source(vec![1, 2, 3, 4, 5])
    ///     .batch(2)
    ///     .run()
    ///     .expect("run");
    /// assert_eq!(out, vec![vec![1, 2], vec![3, 4], vec![5]]);
    /// ```
    #[must_use]
    pub fn batch(self, size: usize) -> Pipeline<Vec<T>> {
        let batch_size = size.max(1);
        let buffer_size = self.buffer_size;
        let prev_spawner = self.spawner;
        let mut names = self.stage_names;
        names.push(format!("batch({})", batch_size));
        Pipeline {
            spawner: Box::new(move |out_sender: Sender<Vec<T>>| {
                let (in_sender, mut receiver) = Channel::new::<T>(buffer_size);
                let _prev_handle = prev_spawner(in_sender);
                tokio::spawn(async move {
                    let mut buf: Vec<T> = Vec::with_capacity(batch_size);
                    while let Some(item) = receiver.recv().await {
                        buf.push(item);
                        if buf.len() >= batch_size {
                            let filled =
                                std::mem::replace(&mut buf, Vec::with_capacity(batch_size));
                            if out_sender.send(filled).await.is_err() {
                                return;
                            }
                        }
                    }
                    if !buf.is_empty() {
                        let _ = out_sender.send(buf).await;
                    }
                })
            }),
            buffer_size,
            stage_names: names,
        }
    }

    /// Collect every `size` items into a `Vec<T>`, apply `reduce`, and
    /// emit the reduced value.
    ///
    /// Like [`Pipeline::batch`] but the batch is reduced to a single
    /// value before being sent downstream. Useful for windowed
    /// aggregations (sum / mean / max over a sliding or tumbling
    /// window). The final partial window is reduced and emitted
    /// in the same way as `batch`.
    ///
    /// Output order is preserved (single-task stage).
    ///
    /// # Example
    ///
    /// ```rust
    /// use buff_pipeline::Pipeline;
    ///
    /// let out = Pipeline::new()
    ///     .source(vec![1, 2, 3, 4, 5])
    ///     .window(2, |batch: Vec<i32>| batch.iter().sum::<i32>())
    ///     .run()
    ///     .expect("run");
    /// assert_eq!(out, vec![3, 7, 5]); // [1+2, 3+4, 5]
    /// ```
    #[must_use]
    pub fn window<U, F>(self, size: usize, reduce: F) -> Pipeline<U>
    where
        U: Send + 'static,
        F: Fn(Vec<T>) -> U + Send + Sync + 'static,
    {
        let window_size = size.max(1);
        let buffer_size = self.buffer_size;
        let prev_spawner = self.spawner;
        let mut names = self.stage_names;
        names.push(format!("window({})", window_size));
        Pipeline {
            spawner: Box::new(move |out_sender: Sender<U>| {
                let (in_sender, mut receiver) = Channel::new::<T>(buffer_size);
                let _prev_handle = prev_spawner(in_sender);
                tokio::spawn(async move {
                    let mut buf: Vec<T> = Vec::with_capacity(window_size);
                    while let Some(item) = receiver.recv().await {
                        buf.push(item);
                        if buf.len() >= window_size {
                            let filled =
                                std::mem::replace(&mut buf, Vec::with_capacity(window_size));
                            let reduced = reduce(filled);
                            if out_sender.send(reduced).await.is_err() {
                                return;
                            }
                        }
                    }
                    if !buf.is_empty() {
                        let reduced = reduce(buf);
                        let _ = out_sender.send(reduced).await;
                    }
                })
            }),
            buffer_size,
            stage_names: names,
        }
    }

    /// Spawn `workers` parallel tasks that each consume from a
    /// round-robin-distributed input channel.
    ///
    /// A single **dispatcher** task drains the input channel and
    /// round-robins items into `workers` distinct worker input channels
    /// (one per worker, per the T14 spec: "workers consume single
    /// Channel each"). Each worker applies `transform` to its received
    /// items and sends the result to a **shared** (cloned) output
    /// sender. A supervisor task awaits the dispatcher + every worker,
    /// then drops the final output sender to signal downstream.
    ///
    /// # Ordering
    ///
    /// Output order is **NOT** preserved under `parallel` — workers
    /// race and the dispatcher does not track per-worker completion
    /// order. Callers that need ordered output should use
    /// [`Pipeline::map`] (single-task) or sort the output `Vec` after
    /// `run()`.
    ///
    /// # Backpressure
    ///
    /// The dispatcher's send to each worker channel is bounded by
    /// `buffer_size`, so a slow worker backpressures the dispatcher
    /// (and transitively the upstream source). The workers' sends to
    /// the shared output channel are also bounded, giving end-to-end
    /// backpressure.
    ///
    /// # Example
    ///
    /// ```rust
    /// use buff_pipeline::Pipeline;
    ///
    /// let out = Pipeline::new()
    ///     .source(vec![1, 2, 3, 4, 5, 6, 7, 8])
    ///     .parallel(4, |x| x * x)
    ///     .run()
    ///     .expect("run");
    /// let mut sorted = out;
    /// sorted.sort();
    /// assert_eq!(sorted, vec![1, 4, 9, 16, 25, 36, 49, 64]);
    /// ```
    #[must_use]
    pub fn parallel<U, F>(self, workers: usize, transform: F) -> Pipeline<U>
    where
        U: Send + 'static,
        F: Fn(T) -> U + Send + Sync + Clone + 'static,
    {
        let n = workers.max(1);
        let buffer_size = self.buffer_size;
        let prev_spawner = self.spawner;
        let mut names = self.stage_names;
        names.push(format!("parallel({})", n));
        Pipeline {
            spawner: Box::new(move |out_sender: Sender<U>| {
                let (in_sender, dispatcher_receiver) = Channel::new::<T>(buffer_size);
                let _prev_handle = prev_spawner(in_sender);

                // Build N worker input channels (one Sender + Receiver pair
                // per worker, per the T14 spec: "workers consume single
                // Channel each").
                let worker_pairs: Vec<(Sender<T>, buff_lang_runtime::Receiver<T>)> =
                    (0..n).map(|_| Channel::new(buffer_size)).collect();
                let worker_senders: Vec<Sender<T>> = worker_pairs
                    .iter()
                    .map(|(s, _)| Sender(s.0.clone()))
                    .collect();
                let worker_receivers: Vec<buff_lang_runtime::Receiver<T>> =
                    worker_pairs.into_iter().map(|(_, r)| r).collect();

                // Dispatcher task: round-robin items into worker_senders.
                let dispatcher = tokio::spawn(async move {
                    let mut receiver = dispatcher_receiver;
                    let mut idx = 0;
                    while let Some(item) = receiver.recv().await {
                        if worker_senders[idx].send(item).await.is_err() {
                            // Worker channel closed (worker died or downstream
                            // closed early). Drop the item and continue;
                            // remaining workers still need feeding.
                        }
                        idx = (idx + 1) % n;
                    }
                    // worker_senders dropped here → every worker sees None
                });

                // Spawn N worker tasks.
                let mut worker_handles: Vec<JoinHandle<()>> = Vec::with_capacity(n);
                for wr in worker_receivers {
                    let transform = transform.clone();
                    // Clone the Sender via the inner tokio mpsc Sender (which
                    // is Clone without requiring T: Clone — the buff-lang-
                    // runtime derive-Clone on Sender<T> adds a T: Clone bound
                    // that is too strict for our generic use case).
                    let sender_clone = Sender(out_sender.0.clone());
                    worker_handles.push(tokio::spawn(async move {
                        let mut wr = wr;
                        while let Some(item) = wr.recv().await {
                            let out = transform(item);
                            if sender_clone.send(out).await.is_err() {
                                return;
                            }
                        }
                    }));
                }

                // Supervisor: wait for dispatcher + all workers, then drop
                // the final output sender to signal downstream.
                tokio::spawn(async move {
                    let _ = dispatcher.await;
                    for handle in worker_handles {
                        let _ = handle.await;
                    }
                    // out_sender dropped here → downstream recv() returns None
                })
            }),
            buffer_size,
            stage_names: names,
        }
    }
}
