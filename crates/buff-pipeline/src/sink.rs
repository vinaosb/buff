//! [`Sink`] — factory namespace for terminal pipeline sinks.
//!
//! Sinks are the "end" of a pipeline: they consume a `Vec<T>` produced
//! by [`Pipeline::run`](crate::Pipeline::run) and either write it to a
//! file or hand it back to the caller as a `Vec`.
//!
//! # Why a unit struct
//!
//! Same namespace-only stance as [`crate::Source`] and
//! [`buff_lang_runtime::Channel`]: `Sink` is NOT a runtime value — it
//! exists purely as a path anchor for `Sink::to_csv` / `to_json` /
//! `collect`. Buff surfaces it as the prelude type `Sink`.

use std::path::Path;

use crate::error::{PipelineError, PipelineResult};
use crate::pipeline::Pipeline;

/// Factory namespace for terminal pipeline sinks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sink;

impl Sink {
    /// Run a pipeline and return the collected output as a `Vec<T>`.
    ///
    /// Sugar for [`Pipeline::run`]: equivalent to `pipeline.run()`.
    /// Provided so Buff codegen can lower `result = p.run().collect()`
    /// into a single `Sink::collect(pipeline)` call site.
    ///
    /// # Example
    ///
    /// ```rust
    /// use buff_pipeline::{Pipeline, Sink};
    ///
    /// let p = Pipeline::new().source(vec![1, 2, 3]).map(|x| x * 2);
    /// let out = Sink::collect(p).expect("collect");
    /// assert_eq!(out, vec![2, 4, 6]);
    /// ```
    pub fn collect<T: Send + 'static>(pipeline: Pipeline<T>) -> PipelineResult<Vec<T>> {
        pipeline.run()
    }

    /// Write `rows` to `path` as a CSV file (no header row).
    ///
    /// Uses [`csv::WriterBuilder`] with `has_headers(false)` so the
    /// output matches the row-preserving stance of
    /// [`Source::from_csv`](crate::Source::from_csv) and the
    /// buff-dataframe CSV module. Each inner `Vec<String>` becomes one
    /// CSV record; string fields are auto-quoted by the `csv` crate if
    /// they contain commas, quotes, or newlines.
    ///
    /// # Errors
    ///
    /// Returns [`PipelineError::Io`] if the file cannot be created or
    /// flushed, or [`PipelineError::Csv`] if a record cannot be
    /// serialized.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use buff_pipeline::Sink;
    ///
    /// # fn main() -> buff_pipeline::PipelineResult<()> {
    /// let rows = vec![
    ///     vec!["name".to_string(), "age".to_string()],
    ///     vec!["Ada".to_string(), "36".to_string()],
    /// ];
    /// Sink::to_csv("people.csv", rows)?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn to_csv(path: impl AsRef<Path>, rows: Vec<Vec<String>>) -> PipelineResult<()> {
        let file = std::fs::File::create(path.as_ref())?;
        let mut writer = csv::WriterBuilder::new()
            .has_headers(false)
            .from_writer(file);
        for row in rows {
            writer
                .write_record(&row)
                .map_err(|e| PipelineError::Csv {
                    detail: e.to_string(),
                })?;
        }
        writer
            .flush()
            .map_err(|e| PipelineError::Csv {
                detail: e.to_string(),
            })?;
        Ok(())
    }

    /// Write `rows` to `path` as a pretty-printed JSON array.
    ///
    /// Uses [`serde_json::to_writer_pretty`] so the output is human-
    /// readable (2-space indentation, sorted keys for serde::Map types).
    /// The input type `T` must implement [`serde::Serialize`].
    ///
    /// # Errors
    ///
    /// Returns [`PipelineError::Io`] if the file cannot be created, or
    /// [`PipelineError::Json`] if serialization fails (rare for plain
    /// types — usually indicates a custom `Serialize` impl that errors).
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use buff_pipeline::Sink;
    /// use serde::Serialize;
    ///
    /// # fn main() -> buff_pipeline::PipelineResult<()> {
    /// #[derive(Serialize)]
    /// struct Person { name: String, age: u32 }
    ///
    /// let people = vec![
    ///     Person { name: "Ada".into(), age: 36 },
    ///     Person { name: "Alan".into(), age: 41 },
    /// ];
    /// Sink::to_json("people.json", &people)?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn to_json<S>(path: impl AsRef<Path>, rows: &S) -> PipelineResult<()>
    where
        S: serde::Serialize + ?Sized,
    {
        let file = std::fs::File::create(path.as_ref())?;
        serde_json::to_writer_pretty(file, rows)?;
        Ok(())
    }
}

impl Default for Sink {
    fn default() -> Self {
        Sink
    }
}
