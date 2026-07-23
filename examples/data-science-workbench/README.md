# Buff Data Science Workbench (T23 Flagship)

The flagship demo for Buff v1.23.0 Wave 11 -- integrates **five frameworks**
end-to-end in one application:

| Framework | Task | Role in the workbench |
|---|---|---|
| `buff-dataframe` | T7 | CSV load + relational preprocessing (filter / group_by / agg) |
| `buff-ml` | T15 | Linear regression training + inference (Model + Tape + SGD + mse_loss) |
| `buff-pipeline` | T14 | Streaming ETL + batched retrain (Source.from_csv -> filter -> batch) |
| `buff-web` | T17 | HTTP endpoints (GET /, /predict, /data, /health; POST /retrain) |
| `buff-reactive` | T20 | `Signal<Model>` for live retrain updates visible to web handlers |
| `buff-observe` | T21 | Span-traced request flow (optional, in server.buff) |

## What it demonstrates

A complete data-science loop in idiomatic Buff:

1. **Load** -- `data_loader.buff` reads `data.csv` (synthetic Iris-like, 30 rows,
   3 species x 10) into a `DataFrame` via `DataFrame.from_csv`.
2. **Analyze** -- relational ops: `filter`, `group_by`, `agg`. The
   `mean_petal_length_by_species` helper exercises the string-form `AggOp`
   syntax (`agg(col, "mean")`).
3. **Train** -- `model.buff` fits a single `Linear(4 -> 1)` layer to predict the
   species code (0=setosa, 1=versicolor, 2=virginica) from the 4 measurements,
   using `SGD` + `mse_loss` over 200 epochs (mirrors the `buff-ml` doc-example).
4. **Orchestrate** -- `pipeline.buff` streams the CSV through
   `Source.from_csv -> filter -> batch -> run` and retrains a fresh model on the
   streamed batches. Invoked by `POST /retrain`.
5. **Serve** -- `server.buff` exposes JSON endpoints reading the live model from
   a `Signal<Model>` so retrains publish instantly.

## File structure

```
data-science-workbench/
+- main.buff           # entry point: load -> preprocess -> train -> serve
+- data_loader.buff    # CSV -> DataFrame + preprocessing + rows_to_json helper
+- model.buff          # Linear(4,1) regression: train_step / train_model / predict
+- pipeline.buff       # Source.from_csv -> filter -> batch -> train (retrain flow)
+- server.buff         # Web.new + GET / + /predict + /data + /health + POST /retrain
+- data.csv            # 30 rows synthetic Iris-like (3 species x 10)
\- README.md           # this file
```

## Endpoints

| Method | Path | Returns |
|---|---|---|
| `GET` | `/` | `{ model_summary: { samples, features, trained, weights }, endpoints }` |
| `GET` | `/predict?sepal_length=5.1&sepal_width=3.5&petal_length=1.4&petal_width=0.2` | `{ prediction, confidence, raw, features }` |
| `POST` | `/retrain` | `{ retrained, new_loss, samples, pipeline_path }` |
| `GET` | `/data` | `{ count, columns, rows }` (T22-FU1 `rows_to_json` roundtrip) |
| `GET` | `/health` | `{ status: "ok" }` |

### Example responses

```json
// GET /predict?sepal_length=5.1&sepal_width=3.5&petal_length=1.4&petal_width=0.2
{
  "prediction": "setosa",
  "confidence": 0.92,
  "raw": 0.08,
  "features": [5.1, 3.5, 1.4, 0.2]
}
```

```json
// POST /retrain
{
  "retrained": true,
  "new_loss": 0.034,
  "samples": 30,
  "pipeline_path": "Source.from_csv -> filter -> batch -> train"
}
```

## How to run (eventually)

```bash
cargo run -p buff-lang-cli -- run examples/data-science-workbench/main.buff
# [server] listening on http://0.0.0.0:8080
```

Then:

```bash
curl http://localhost:8080/
curl "http://localhost:8080/predict?sepal_length=5.1&sepal_width=3.5&petal_length=1.4&petal_width=0.2"
curl -X POST http://localhost:8080/retrain
curl http://localhost:8080/data
```

## [!] Current limitation -- codegen deferred

**End-to-end execution via `buff run` is NOT yet possible.** Per the
[T22 API compatibility report](../../.sisyphus/decisions/api-compat-v20.md):

> The `Type::{DataFrame, Tensor, Pipeline, Signal, Computed, Effect, Web}`
> variants in `crates/buff-lang-types/src/ty.rs` plus their codegen lowering
> arms in `crates/buff-lang-codegen-rust/src/rust_codegen.rs` are coordinated
> sibling tasks outside each framework crate's shared zone.

These `.buff` files therefore serve as:

- **Flagship demo** -- concrete composition of all five frameworks in idiomatic Buff.
- **API documentation** -- reference usage for each framework's public surface.
- **Integration-test design** -- when the codegen layer lands, these files become
  the end-to-end runnable test bed.
- **Parse validation** -- `buff check` validates syntax (the files mirror the
  proven patterns in `examples/integration/*.buff`, which parse cleanly).

### T22 workarounds applied

These files apply every documented workaround from the T22 mismatch report:

| ID | Workaround | Where |
|---|---|---|
| **T22-FU1** | `rows_to_json(df)` helper (no native `DataFrame.to_json()` exists) | `data_loader.buff`, `server.buff` `/data` |
| **T22-FU2** | Tensor instance methods documented as `Type::Unknown`; minimised direct Tensor calls, lean on `Model.forward` | `model.buff` |
| **T22-FU3** | `DataFrame.from_rows(headers: [...], rows: [[...]])` named-arg call shape (convention section 11) | `pipeline.buff`, `data_loader.buff` |
| **T22-FU4** | String-form `AggOp`: `agg(col, "mean")` per buff-dataframe AGENTS.md | `data_loader.buff::mean_petal_length_by_species` |
| **T22-FU5** | Single-threaded tokio runtime (`new_current_thread()`) for `Signal<T>` + `Web` sharing -- documented in `server.buff` header | `server.buff` |

## Architecture

```
                 +---------------------------------------------+
                 |                  main.buff                   |
                 |  load_data -> preprocess -> train -> serve   |
                 +----------------------+----------------------+
                                        |
        +-------------------------------+-------------------------------+
        |                               |                               |
        v                               v                               v
+---------------+          +-----------------+        +------------------+
| data_loader   |          |     model       |        |     server       |
|  DataFrame    |          |  Model/Tape/SGD |        |   Web + Signal   |
|  rows_to_json |          |  predict/train  |        |   + buff-observe |
+-------+-------+          +--------+--------+        +--------+---------+
        |                           |                          |
        |           POST /retrain   |     reads Signal<Model>  |
        |      +--------------------+                          |
        v      v                                               v
+-------------------------+                       +-----------------------+
|       pipeline          |  retrain flow         |   Signal<Model>       |
|  Source.from_csv        | --------------------> |   (T22-FU5:           |
|  -> filter -> batch     |   publishes fresh     |    single-threaded    |
|  -> train               |   model via set()     |    tokio runtime)     |
+-------------------------+                       +-----------------------+
```

The **signal-driven model** is the architectural centrepiece: `POST /retrain`
runs the streaming pipeline, trains a fresh model, and calls
`model_signal.set(trained)`. Every subsequent `GET /` and `GET /predict`
observes the updated weights -- no restart needed. This composes
`buff-reactive` (T20) + `buff-web` (T17) + `buff-pipeline` (T14) + `buff-ml`
(T15) + `buff-dataframe` (T7) in one reactive loop.

## Dataset

`data.csv` is a 30-row synthetic subset of the classic
[Iris dataset](https://archive.ics.uci.edu/ml/datasets/iris), 10 rows per
species (`setosa` / `versicolor` / `virginica`). Columns:

```
sepal_length, sepal_width, petal_length, petal_width, species
```

## License

MIT OR Apache-2.0 (same as the rest of the Buff workspace).
