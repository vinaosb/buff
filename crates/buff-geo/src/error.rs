//! Error type for the `buff-geo` crate.
//!
//! All fallible operations surface as [`GeoError`]. The crate's public
//! constructors (`Point::new` / `LineString::new` / `LineString::from_coords`
//! / `Polygon::new` / `Polygon::from_coords`) map upstream `geo_*` errors
//! into this enum so the crate's public surface depends only on `buff-geo`'s
//! own types (Buff code never sees a raw `geo::*` type).
//!
//! # Panic-free contract
//!
//! No `unwrap` / `expect` / `panic!` / `todo!` / `unimplemented!` in
//! this module or any non-test code path. Per the T4 FFI guide R6
//! (Panic Boundary) the public entry points that wrap potentially-
//! panicking `geo` algorithm calls use `catch_unwind` so panics never
//! propagate across the FFI boundary into Buff code.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum GeoError {
    /// `LineString::new` / `LineString::from_coords` / `Polygon::new` /
    /// `Polygon::from_coords` called with an empty coordinate slice.
    /// The upstream `geo_types::LineString::from` silently accepts an
    /// empty vec but every algorithm produces trivial / zero results;
    /// we surface an explicit error so the Buff user sees their bug.
    #[error("geo coordinate list is empty")]
    EmptyCoords,

    /// `LineString::from_coords` / `Polygon::from_coords` called with
    /// an odd number of floats. The flat `[x1, y1, x2, y2, ...]`
    /// convention requires an even count.
    #[error("geo coordinate list has odd length {len}; expected (x, y) pairs")]
    OddCoords { len: usize },

    /// `Polygon::new` / `Polygon::from_coords` called with fewer than
    /// 3 distinct vertices. A degenerate polygon has no interior; the
    /// upstream `geo::algorithm::Area` would return 0 but every other
    /// op (Contains / Intersects / BooleanOps) is undefined.
    #[error("polygon needs >= 3 distinct vertices, got {n}")]
    DegeneratePolygon { n: usize },

    /// `Projection::wgs84_to_web_mercator` called with a latitude
    /// outside the Web Mercator valid range ([-85.05112878, 85.05112878]).
    /// The projection is mathematically undefined at the poles; the
    /// upstream `geo::algorithm::webmercator` would panic — we surface
    /// the explicit error instead per FFI guide R6.
    #[error("latitude {lat} out of Web Mercator range [-85.05.., 85.05..]")]
    LatitudeOutOfRange { lat: f64 },

    /// A wrapper-internal panic was caught by `catch_unwind` (per
    /// T4 FFI guide R6). The user sees a stable diagnostic instead
    /// of a process abort.
    #[error("internal error: geo operation panicked")]
    Panic,
}
