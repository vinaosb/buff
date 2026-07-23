//! Integration tests for `buff-pipeline`.
//!
//! Plain `#[test]` functions (NOT `#[tokio::test]`) — `Pipeline::run`
//! builds its own multi-thread tokio runtime internally. Calling
//! `.run()` from inside another runtime panics with "Cannot start a
//! runtime from within a runtime", so we drive everything from plain
//! sync test bodies.
//!
//! Coverage:
//!  * Simple map/filter pipeline (T14 acceptance scenario #1).
//!  * Source pushes via Channel<T>, stages consume correctly.
//!  * Sink side-effect + collect round-trip.
//!  * parallel stage spawns N workers, output set preserved.
//!  * batch / window / filter edge cases.
//!  * CSV source streaming + Sink::to_csv / to_json round-trips.
//!  * 5+ inline insta snapshots.

use buff_pipeline::{Pipeline, Sink, Source};
use std::sync::{Arc, Mutex};

// ---------------------------------------------------------------------------
// Acceptance scenario #1: simple map-filter pipeline.
// ---------------------------------------------------------------------------

#[test]
fn simple_map_filter_yields_expected_output() {
    // T14 acceptance scenario: [1, 2, 3, 4, 5] → map(*2) → filter(>4) → [6, 8, 10]
    let result = Pipeline::new()
        .source(vec![1, 2, 3, 4, 5])
        .map(|x| x * 2)
        .filter(|x| *x > 4)
        .run()
        .expect("pipeline should run");
    assert_eq!(result, vec![6, 8, 10]);
}

#[test]
fn source_pushes_via_channel_and_stages_consume() {
    // Verify items survive the source → stage → drain round-trip.
    let result = Pipeline::new()
        .source(vec!["alpha".to_string(), "beta".to_string(), "gamma".to_string()])
        .stage("uppercase", |s: String| s.to_uppercase())
        .run()
        .expect("run");
    assert_eq!(
        result,
        vec!["ALPHA".to_string(), "BETA".to_string(), "GAMMA".to_string()]
    );
}

// ---------------------------------------------------------------------------
// sink + collect.
// ---------------------------------------------------------------------------

#[test]
fn sink_collect_returns_vec_in_order() {
    let observed: Arc<Mutex<Vec<i32>>> = Arc::new(Mutex::new(Vec::new()));
    let obs_clone = observed.clone();
    let result = Pipeline::new()
        .source(vec![1, 2, 3])
        .sink(move |x: &i32| {
            if let Ok(mut guard) = obs_clone.lock() {
                guard.push(*x);
            }
        })
        .run()
        .expect("run");
    // The pipeline output preserves order...
    assert_eq!(result, vec![1, 2, 3]);
    // ...and the sink side-effect observed every item.
    let snapshot = observed.lock().expect("lock").clone();
    assert_eq!(snapshot, vec![1, 2, 3]);
}

#[test]
fn sink_collect_helper_runs_pipeline() {
    let p = Pipeline::new().source(vec![1, 2, 3]).map(|x| x * 2);
    let out = Sink::collect(p).expect("collect");
    assert_eq!(out, vec![2, 4, 6]);
}

// ---------------------------------------------------------------------------
// parallel workers.
// ---------------------------------------------------------------------------

#[test]
fn parallel_stage_spawns_workers_output_preserved_as_set() {
    // Output order is NOT preserved under `parallel` (workers race) —
    // verify the SET of outputs matches by sorting before comparison.
    let result = Pipeline::new()
        .source(vec![1, 2, 3, 4, 5, 6, 7, 8])
        .parallel(4, |x| x * x)
        .run()
        .expect("run");
    let mut sorted = result;
    sorted.sort();
    assert_eq!(sorted, vec![1, 4, 9, 16, 25, 36, 49, 64]);
}

#[test]
fn parallel_with_one_worker_preserves_order() {
    // With workers=1 the dispatcher round-robins into a single worker
    // → effectively sequential → order IS preserved.
    let result = Pipeline::new()
        .source(vec![1, 2, 3, 4, 5])
        .parallel(1, |x| x + 100)
        .run()
        .expect("run");
    assert_eq!(result, vec![101, 102, 103, 104, 105]);
}

// ---------------------------------------------------------------------------
// batch / window.
// ---------------------------------------------------------------------------

#[test]
fn batch_stage_groups_n_items() {
    let result = Pipeline::new()
        .source(vec![1, 2, 3, 4, 5])
        .batch(2)
        .run()
        .expect("run");
    assert_eq!(result, vec![vec![1, 2], vec![3, 4], vec![5]]);
}

#[test]
fn batch_size_larger_than_input_emits_single_partial_batch() {
    let result = Pipeline::new().source(vec![1, 2, 3]).batch(10).run().expect("run");
    assert_eq!(result, vec![vec![1, 2, 3]]);
}

#[test]
fn window_stage_reduces() {
    let result = Pipeline::new()
        .source(vec![1, 2, 3, 4, 5])
        .window(2, |batch: Vec<i32>| batch.iter().sum::<i32>())
        .run()
        .expect("run");
    assert_eq!(result, vec![3, 7, 5]); // [1+2, 3+4, 5]
}

// ---------------------------------------------------------------------------
// Edge cases.
// ---------------------------------------------------------------------------

#[test]
fn empty_pipeline_returns_empty_vec() {
    let out: Vec<i32> = Pipeline::new().source(Vec::new()).run().expect("run");
    assert!(out.is_empty());
}

#[test]
fn filter_rejects_everything_yields_empty_vec() {
    let out = Pipeline::new()
        .source(vec![1, 2, 3])
        .filter(|_x| false)
        .run()
        .expect("run");
    assert!(out.is_empty());
}

#[test]
fn chained_stages_compose_in_order() {
    let result = Pipeline::new()
        .source(vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10])
        .filter(|x| *x % 2 == 0) // [2, 4, 6, 8, 10]
        .map(|x| x * 10)        // [20, 40, 60, 80, 100]
        .filter(|x| *x < 70)    // [20, 40, 60]
        .map(|x| x / 10)        // [2, 4, 6]
        .run()
        .expect("run");
    assert_eq!(result, vec![2, 4, 6]);
}

#[test]
fn with_buffer_override_does_not_break_pipeline() {
    let result = Pipeline::new()
        .source(vec![1, 2, 3])
        .with_buffer(1) // rendezvous-ish: tightest possible backpressure
        .map(|x| x + 1)
        .run()
        .expect("run");
    assert_eq!(result, vec![2, 3, 4]);
}

// ---------------------------------------------------------------------------
// CSV source streaming.
// ---------------------------------------------------------------------------

#[test]
fn csv_source_reads_chunks_correctly() {
    let dir = tempfile::tempdir().expect("tempdir");
    let csv_path = dir.path().join("input.csv");
    std::fs::write(
        &csv_path,
        "alice,30,nyc\nbob,25,la\ncarol,40,sf\n",
    )
    .expect("write csv");

    let rows = Source::from_csv(&csv_path, 2)
        .expect("open csv")
        .run()
        .expect("run");

    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0], vec!["alice".to_string(), "30".into(), "nyc".into()]);
    assert_eq!(rows[2], vec!["carol".to_string(), "40".into(), "sf".into()]);
}

#[test]
fn csv_source_missing_file_returns_io_error() {
    let err = Source::from_csv("does/not/exist.csv", 10).unwrap_err();
    assert!(matches!(err, buff_pipeline::PipelineError::Io { .. }));
    assert!(err.to_string().contains("not found"));
}

#[test]
fn csv_to_csv_round_trip_preserves_data() {
    let dir = tempfile::tempdir().expect("tempdir");
    let in_path = dir.path().join("in.csv");
    let out_path = dir.path().join("out.csv");

    let original = "alpha,1\nbeta,2\ngamma,3\n";
    std::fs::write(&in_path, original).expect("write");

    let rows = Source::from_csv(&in_path, 2)
        .expect("read")
        .map(|row: Vec<String>| {
            // Prepend "x_" to the first column to prove the transform ran.
            let mut r = row;
            if let Some(first) = r.get_mut(0) {
                *first = format!("x_{}", first);
            }
            r
        })
        .run()
        .expect("run");

    Sink::to_csv(&out_path, rows).expect("write csv");

    let written = std::fs::read_to_string(&out_path).expect("read back");
    assert!(written.contains("x_alpha,1"));
    assert!(written.contains("x_beta,2"));
    assert!(written.contains("x_gamma,3"));
}

// ---------------------------------------------------------------------------
// JSON sink.
// ---------------------------------------------------------------------------

#[test]
fn json_sink_writes_pretty_array() {
    let dir = tempfile::tempdir().expect("tempdir");
    let json_path = dir.path().join("out.json");

    let data = vec![1, 2, 3];
    Sink::to_json(&json_path, &data).expect("write json");

    let written = std::fs::read_to_string(&json_path).expect("read");
    assert!(written.contains("["));
    assert!(written.contains("1"));
    assert!(written.contains("2"));
    assert!(written.contains("3"));
    assert!(written.contains("]"));
}

// ---------------------------------------------------------------------------
// Snapshots (5+ inline insta snapshots).
// ---------------------------------------------------------------------------

#[test]
fn snap_pipeline_debug_repr() {
    let p = Pipeline::new()
        .source(vec![1, 2, 3])
        .map(|x| x * 2)
        .filter(|x| *x > 2)
        .parallel(2, |x| x + 1);
    insta::assert_snapshot!(
        format!("{:?}", p),
        @r###"
    Pipeline {
        stages: [
            "source",
            "map",
            "filter",
            "parallel(2)",
        ],
        buffer_size: 64,
        ..
    }
    "###
    );
}

#[test]
fn snap_map_filter_output() {
    let result = Pipeline::new()
        .source(vec![1, 2, 3, 4, 5])
        .map(|x| x * 2)
        .filter(|x| *x > 4)
        .run()
        .expect("run");
    insta::assert_snapshot!(format!("{:?}", result), @"[6, 8, 10]");
}

#[test]
fn snap_batch_output() {
    let result = Pipeline::new()
        .source(vec![1, 2, 3, 4, 5, 6, 7])
        .batch(3)
        .run()
        .expect("run");
    insta::assert_snapshot!(
        format!("{:?}", result),
        @"[[1, 2, 3], [4, 5, 6], [7]]"
    );
}

#[test]
fn snap_window_sum_output() {
    let result = Pipeline::new()
        .source(vec![1, 2, 3, 4, 5, 6])
        .window(2, |batch: Vec<i32>| batch.iter().sum::<i32>())
        .run()
        .expect("run");
    insta::assert_snapshot!(format!("{:?}", result), @"[3, 7, 11]");
}

#[test]
fn snap_csv_round_trip_content() {
    let dir = tempfile::tempdir().expect("tempdir");
    let in_path = dir.path().join("snap_in.csv");
    let out_path = dir.path().join("snap_out.csv");

    std::fs::write(&in_path, "a,1\nb,2\nc,3\n").expect("write");

    let rows = Source::from_csv(&in_path, 8)
        .expect("read")
        .filter(|r: &Vec<String>| r[1] != "2")
        .run()
        .expect("run");
    Sink::to_csv(&out_path, rows).expect("write csv");

    let written = std::fs::read_to_string(&out_path).expect("read back");
    insta::assert_snapshot!(written, @"a,1\nc,3\n");
}

// ---------------------------------------------------------------------------
// Property tests (proptest).
// ---------------------------------------------------------------------------

proptest::proptest! {
    #[test]
    fn map_then_filter_preserves_predicate(input in proptest::collection::vec(1i32..1000, 0..100)) {
        let doubled: Vec<i32> = input.iter().map(|x| x * 2).filter(|x| *x > 100).collect();
        let piped = Pipeline::new()
            .source(input)
            .map(|x| x * 2)
            .filter(|x| *x > 100)
            .run()
            .expect("run");
        prop_assert_eq!(piped, doubled);
    }

    #[test]
    fn batch_preserves_item_count(input in proptest::collection::vec(1i32..1000, 0..50)) {
        let total: usize = Pipeline::new()
            .source(input.clone())
            .batch(3)
            .run()
            .expect("run")
            .iter()
            .map(Vec::len)
            .sum();
        prop_assert_eq!(total, input.len());
    }
}
