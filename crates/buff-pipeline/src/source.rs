//! [`Source`] — factory namespace for streaming pipeline sources.
//!
//! Currently ships a single source ([`Source::from_csv`]). Future
//! sources (Kafka / Redis Streams / HTTP / etc.) are deferred to
//! v1.18+ per the T14 task spec.
//!
//! # Why a unit struct + inherent associated function
//!
//! Mirrors the namespace-only stance used by [`buff_lang_runtime::Channel`]
//! and the buff-cache [`Cache`](buff_lang_runtime) pattern: `Source` is
//! NOT a runtime value — it exists purely as a path anchor for
//! `Source::from_csv`. Buff surfaces it as the prelude type `Source`;
//! users write `Source.from_csv(path, chunk_size: 100)`.

use std::path::Path;

use buff_lang_runtime::Sender;
use rayon::prelude::*;

use crate::error::{PipelineError, PipelineResult};
use crate::pipeline::Pipeline;

/// Factory namespace for pipeline sources.
///
/// `Source` itself carries no state — it is a unit struct used solely
/// as a path anchor for the inherent associated function
/// [`Source::from_csv`]. Construct sources via the typed call; do NOT
/// hold a `Source` value at runtime (it has no behavior).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Source;

impl Source {
    /// Stream a CSV file into a pipeline, row by row, with bounded memory.
    ///
    /// Opens `path`, parses it via [`csv::ReaderBuilder`] with
    /// `has_headers(false)` (header row is preserved in the record
    /// stream — matches the buff-dataframe + Csv module stance), and
    /// pushes each row as a `Vec<String>` into the pipeline's first
    /// inter-stage channel.
    ///
    /// # Streaming + backpressure
    ///
    /// Reading happens inside a `tokio::task::spawn_blocking` task so
    /// the sync `csv::Reader` does NOT block the async runtime. Records
    /// are read in chunks of `chunk_size`; each chunk is parsed in
    /// parallel via [`rayon::par_iter`] (near-linear speedup on multi-
    /// core hosts for the `StringRecord -> Vec<String>` conversion).
    /// Each parsed row is then pushed into the channel via
    /// [`Sender::blocking_send`] (the sync variant of `tokio::mpsc::
    /// Sender::send`), which parks the blocking thread when the
    /// channel is full — this is the **natural backpressure** mechanism
    /// that keeps memory bounded regardless of CSV size.
    ///
    /// # Arguments
    ///
    /// * `path`     — CSV file path. Any `AsRef<Path>` (String, &str,
    ///   PathBuf, etc.).
    /// * `chunk_size` — Number of CSV records to read + parse per
    ///   chunk. Also sets the inter-stage channel buffer capacity for
    ///   the source → first-stage queue. Clamped to `>= 1`. Larger
    ///   values amortize rayon's par_iter overhead; smaller values
    ///   tighten backpressure.
    ///
    /// # Errors
    ///
    /// Returns [`PipelineError::Io`] if `path` does not exist or
    /// cannot be read.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use buff_pipeline::{Source, Sink};
    ///
    /// # fn main() -> buff_pipeline::PipelineResult<()> {
    /// let rows = Source::from_csv("input.csv", 100)?
    ///     .filter(|row: &Vec<String>| row.len() >= 2)
    ///     .map(|row| row.iter().take(2).cloned().collect::<Vec<String>>())
    ///     .run()?;
    /// Sink::to_csv("output.csv", rows)?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn from_csv(
        path: impl AsRef<Path>,
        chunk_size: usize,
    ) -> PipelineResult<Pipeline<Vec<String>>> {
        let path = path.as_ref().to_path_buf();
        if !path.exists() {
            return Err(PipelineError::Io {
                detail: format!("source CSV file not found: {}", path.display()),
            });
        }
        let buffer_size = chunk_size.max(1);
        Ok(Pipeline {
            spawner: Box::new(move |sender: Sender<Vec<String>>| {
                let path = path.clone();
                tokio::task::spawn_blocking(move || {
                    let file = match std::fs::File::open(&path) {
                        Ok(f) => f,
                        Err(_) => return,
                    };
                    let reader = csv::ReaderBuilder::new()
                        .has_headers(false)
                        .from_reader(file);
                    let mut records = reader.into_records();
                    loop {
                        let mut chunk: Vec<csv::StringRecord> =
                            Vec::with_capacity(buffer_size);
                        for _ in 0..buffer_size {
                            match records.next() {
                                Some(Ok(record)) => chunk.push(record),
                                // Skip malformed records — MVP policy is
                                // best-effort (mirrors buff-dataframe's
                                // load_csv stance). A future strict mode
                                // can surface a PipelineError instead.
                                Some(Err(_)) => continue,
                                None => break,
                            }
                        }
                        if chunk.is_empty() {
                            break;
                        }
                        // Parse chunk in parallel — csv::StringRecord is
                        // Send + Sync (Vec<u8> + Vec<usize> underneath)
                        // so par_iter is sound.
                        let rows: Vec<Vec<String>> = chunk
                            .par_iter()
                            .map(|record| {
                                record.iter().map(|cell| cell.to_string()).collect()
                            })
                            .collect();
                        for row in rows {
                            // blocking_send parks this worker thread
                            // until the channel has a free slot — this
                            // is the bounded-memory backpressure.
                            if sender.0.blocking_send(row).is_err() {
                                // Downstream closed early; stop reading.
                                return;
                            }
                        }
                    }
                })
            }),
            buffer_size,
            stage_names: vec!["source(csv)".to_string()],
        })
    }
}

impl Default for Source {
    fn default() -> Self {
        Source
    }
}
