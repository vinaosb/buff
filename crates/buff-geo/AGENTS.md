# buff-geo

Geospatial / GIS primitives for the Buff language. Pure-Rust MVP (CPU-only per Metis G7 lock). Wraps the [`geo`](https://crates.io/crates/geo) + [`geo-types`](https://crates.io/crates/geo-types) crates via a safe FFI boundary per the [T4 FFI guide](../buff-lang-ffi-guide/GUIDE.md).

**Status: experimental** (T45 v1.17 frameworks wave 4).

## STRUCTURE

```
buff-geo/
├── Cargo.toml            # geo + geo-types + thiserror deps
├── src/
│   ├── lib.rs            # Point + LineString + Polygon + Projection (~310 LOC)
│   └── error.rs          # GeoError enum (~55 LOC)
└── tests/
    └── core.rs           # unit tests
```

## WHERE TO LOOK

| Task | File |
|---|---|
| Add a new geometry type | `src/lib.rs` (add struct + impl) + `crates/buff-lang-types/src/prelude_types.rs` + `crates/buff-lang-codegen-rust/src/rust_codegen.rs` |
| Add a new error variant | `src/error.rs` |
| Add a new projection | `src/lib.rs::Projection` impl block |
| Wire a Buff-side method to codegen | `crates/buff-lang-types/src/prelude_types.rs` (PreludeInstanceFn + `instance_fn_return_type`) + `crates/buff-lang-codegen-rust/src/rust_codegen.rs::lower_prelude_type_instance_fn` |

## PUBLIC API

### `Point` — 2D point (f64)

| Method | Signature | Notes |
|---|---|---|
| `Point::new` | `(x: f64, y: f64) -> Point` | Infallible. |
| `point.x` / `point.y` | `(self) -> f64` | Copy type. |
| `point.distance_to` | `(self, other: Point) -> f64` | Euclidean. |
| `point.buffer` | `(self, radius: f64) -> Result<Polygon, GeoError>` | Circle approximation (32 segments). |

### `LineString` — ordered sequence of points

| Method | Signature | Notes |
|---|---|---|
| `LineString::new` | `(Vec<Point>) -> Result<Self, GeoError>` | Empty check. |
| `LineString::from_coords` | `(Vec<f64>) -> Result<Self, GeoError>` | Flat `[x1, y1, x2, y2, ...]`. Odd-length rejected. |
| `ls.length` | `(&self) -> f64` | Euclidean. |
| `ls.num_points` | `(&self) -> usize` | |

### `Polygon` — outer ring + future holes

| Method | Signature | Notes |
|---|---|---|
| `Polygon::new` | `(Vec<Point>) -> Result<Self, GeoError>` | Auto-closes ring. Min 3 vertices. |
| `Polygon::from_coords` | `(Vec<f64>) -> Result<Self, GeoError>` | |
| `poly.area` | `(&self) -> f64` | Unsigned. |
| `poly.contains` | `(&self, Point) -> bool` | |
| `poly.intersects` | `(&self, &Polygon) -> bool` | `catch_unwind` boundary (BooleanOps). |
| `poly.num_vertices` | `(&self) -> usize` | |

### `Projection` — Web Mercator (EPSG:3857)

| Method | Signature | Notes |
|---|---|---|
| `Projection::wgs84_to_web_mercator` | `(Point) -> Result<Point, GeoError>` | Lat range check. |

## CONVENTIONS

- **Pure-Rust only**: wraps `geo` + `geo-types` (both pure-Rust, no cc-rs). NO GEOS / GDAL / PROJ bindings.
- **CPU-only per Metis G7 lock**: NO GPU dispatch.
- **FFI safety**: every public entry point follows the 6 hard rules from `crates/buff-lang-ffi-guide/GUIDE.md`. See the compliance table in `src/lib.rs` module doc.
- **Panic-free**: no `unwrap` / `expect` / `panic!` in non-test code. `Polygon.intersects` / `Polygon.buffer` wrap `catch_unwind` per FFI guide R6.
- **Polygon holes deferred**: the MVP supports only the outer ring (no interior holes). Holes are a v1.18+ enhancement.

## CODEGEN INTEGRATION

The Buff surface (`Point.new(x, y)` / `point.distance_to(q)` / `poly.area()`) is wired in:

- **Type variants**: `Type::Point` / `Type::LineString` / `Type::Polygon` in `crates/buff-lang-types/src/ty.rs`
- **Prelude registry**: `PreludeType::Point` / `LineString` / `Polygon` + `PreludeAssocFn::FromCoords` + `PreludeInstanceFn::{X, Y, DistanceTo, Length, Area, Intersects}` in `crates/buff-lang-types/src/prelude_types.rs`
- **Lowering**: `lower_prelude_type_assoc_fn` + `lower_prelude_type_instance_fn` in `crates/buff-lang-codegen-rust/src/rust_codegen.rs`
- **Extern crates**: `buff-geo` + `geo` + `geo-types` registered via `program_uses_namespace("Point")` / `("LineString")` / `("Polygon")`
- **Tests**: `crates/buff-lang-codegen-rust/tests/geo_codegen.rs`

## DEFERRED

- Polygon interior holes (v1.18+)
- GPU dispatch (Metis G7 lock)
- Additional projections beyond Web Mercator (v1.18+)
- GeoJSON / WKT / WKB serialization (v1.18+)
- Coordinate Reference System transformations (v1.22+)
