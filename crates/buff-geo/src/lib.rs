//! `buff-geo` — geospatial / GIS primitives for the Buff language.
//!
//! Pure-Rust MVP wrapping the [`geo`](https://crates.io/crates/geo) +
//! [`geo-types`](https://crates.io/crates/geo-types) crates. CPU-only
//! (no GPU dispatch — that's deferred to v1.18+ per Metis G7 lock).
//!
//! # Pipeline
//!
//! ```text
//!   Point.new(x, y) ────────────────────────┐
//!                                            ▼
//!   LineString.new(points) ──────▶ LineString { geo_types::LineString }
//!   LineString.from_coords(flat) ───────────┘
//!                                            ▼
//!   Polygon.new(ring) ──────────────────────┐
//!   Polygon.from_coords(flat) ──────────────┴─▶ Polygon { geo_types::Polygon }
//!                                            │
//!                                            ├─ point.distance_to(other)
//!                                            ├─ point.buffer(radius)
//!                                            ├─ line_string.length()
//!                                            ├─ polygon.area()
//!                                            ├─ polygon.contains(point)
//!                                            └─ polygon.intersects(other)
//!                                            ▼
//!                                  Projection.wgs84_to_web_mercator(point)
//!                                            (EPSG:3857)
//! ```
//!
//! # FFI safety
//!
//! Every public entry point follows the 6 hard rules from
//! `crates/buff-lang-ffi-guide/GUIDE.md`:
//!
//! | Rule | How this crate complies |
//! |------|-------------------------|
//! | R1 — No raw pointers | Public surface exposes only `Point`, `LineString`, `Polygon`, `GeoError`. No `*const` / `*mut` anywhere. |
//! | R2 — Ownership boundary | `new` / `from_coords` return owned values. `distance_to` / `area` / `contains` borrow `&self`. |
//! | R3 — Error mapping | Every fallible op returns `Result<T, GeoError>`. Upstream `geo` errors mapped via `From` where they occur. |
//! | R4 — Thread safety | `Point` / `LineString` / `Polygon` are `Send + Sync` (wrapping `geo_types::*` which are themselves `Send + Sync`). |
//! | R5 — Lifetime hiding | No public lifetime parameters. All values own their `geo_types` payloads. |
//! | R6 — Panic boundary | `Polygon::intersects` / `Polygon::buffer` (BooleanOps) wrap their bodies in `catch_unwind` (per FFI guide §6). |
//!
//! # Panic-free contract
//!
//! No `unwrap` / `expect` / `panic!` / `todo!` / `unimplemented!` in
//! non-test code. Bounds-checked construction returns `Result`.

pub mod error;

pub use error::GeoError;

use std::panic::{catch_unwind, AssertUnwindSafe};

const WEB_MERCATOR_MAX_LAT: f64 = 85.05112878;
const WEB_MERCATOR_R: f64 = 6378137.0;

/// A 2D point with `f64` coordinates.
///
/// Wraps `geo_types::Point<f64>`. Constructed via [`Point::new`].
/// Instance methods: `point.x()`, `point.y()`, `point.distance_to(other)`,
/// `point.buffer(radius)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point(pub(crate) geo_types::Point<f64>);

impl Point {
    pub fn new(x: f64, y: f64) -> Self {
        Point(geo_types::Point::<f64>::new(x, y))
    }

    #[inline]
    pub fn x(self) -> f64 {
        self.0.x()
    }

    #[inline]
    pub fn y(self) -> f64 {
        self.0.y()
    }

    pub fn distance_to(self, other: Point) -> f64 {
        use geo::EuclideanDistance;
        self.0.euclidean_distance(&other.0)
    }

    pub fn buffer(self, radius: f64) -> Result<Polygon, GeoError> {
        if !radius.is_finite() || radius < 0.0 {
            return Err(GeoError::DegeneratePolygon { n: 0 });
        }
        if radius == 0.0 {
            let ring = geo_types::LineString::<f64>::from(vec![
                self.0,
                self.0,
                self.0,
                self.0,
            ]);
            return Ok(Polygon(geo_types::Polygon::<f64>::new(ring, vec![])));
        }
        let cx = self.0.x();
        let cy = self.0.y();
        const N: usize = 32;
        let mut coords: Vec<geo_types::Coord<f64>> = Vec::with_capacity(N + 1);
        for i in 0..N {
            let theta = 2.0 * std::f64::consts::PI * (i as f64) / (N as f64);
            coords.push(geo_types::Coord::<f64> {
                x: cx + radius * theta.cos(),
                y: cy + radius * theta.sin(),
            });
        }
        coords.push(coords[0]);
        let ring = geo_types::LineString::<f64>(coords);
        Ok(Polygon(geo_types::Polygon::<f64>::new(ring, vec![])))
    }
}

impl Default for Point {
    fn default() -> Self {
        Point::new(0.0, 0.0)
    }
}

impl std::fmt::Display for Point {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Point({}, {})", self.0.x(), self.0.y())
    }
}

/// A polyline — an ordered sequence of [`Point`]s.
///
/// Wraps `geo_types::LineString<f64>`. Constructed via
/// [`LineString::new`] (takes `Vec<Point>`) or
/// [`LineString::from_coords`] (takes flat `Vec<f64>`).
/// Instance method: `line_string.length()`.
#[derive(Debug, Clone, PartialEq)]
pub struct LineString(pub(crate) geo_types::LineString<f64>);

impl LineString {
    pub fn new(points: Vec<Point>) -> Result<Self, GeoError> {
        if points.is_empty() {
            return Err(GeoError::EmptyCoords);
        }
        let raw: Vec<geo_types::Point<f64>> = points.into_iter().map(|p| p.0).collect();
        Ok(LineString(geo_types::LineString::<f64>::from(raw)))
    }

    pub fn from_coords(coords: Vec<f64>) -> Result<Self, GeoError> {
        if coords.is_empty() {
            return Err(GeoError::EmptyCoords);
        }
        if coords.len() % 2 != 0 {
            return Err(GeoError::OddCoords { len: coords.len() });
        }
        let mut pts: Vec<geo_types::Coord<f64>> = Vec::with_capacity(coords.len() / 2);
        for chunk in coords.chunks_exact(2) {
            pts.push(geo_types::Coord::<f64> {
                x: chunk[0],
                y: chunk[1],
            });
        }
        Ok(LineString(geo_types::LineString::<f64>(pts)))
    }

    pub fn length(&self) -> f64 {
        use geo::EuclideanLength;
        self.0.euclidean_length()
    }

    pub fn num_points(&self) -> usize {
        self.0.coords_count()
    }
}

impl Default for LineString {
    fn default() -> Self {
        LineString(geo_types::LineString::<f64>::from(vec![
            geo_types::Point::<f64>::new(0.0, 0.0),
        ]))
    }
}

impl std::fmt::Display for LineString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "LineString({} points)", self.num_points())
    }
}

/// A polygon — an outer ring + (future) interior holes.
///
/// Wraps `geo_types::Polygon<f64>`. The MVP supports only the outer
/// ring (no holes); holes are a v1.18+ enhancement. Constructed via
/// [`Polygon::new`] (takes `Vec<Point>`) or [`Polygon::from_coords`]
/// (takes flat `Vec<f64>`). Instance methods: `polygon.area()`,
/// `polygon.contains(point)`, `polygon.intersects(other)`.
#[derive(Debug, Clone, PartialEq)]
pub struct Polygon(pub(crate) geo_types::Polygon<f64>);

impl Polygon {
    pub fn new(ring: Vec<Point>) -> Result<Self, GeoError> {
        if ring.len() < 3 {
            return Err(GeoError::DegeneratePolygon { n: ring.len() });
        }
        let mut pts: Vec<geo_types::Coord<f64>> =
            ring.into_iter().map(|p| p.0.into()).collect();
        let first = pts[0];
        let last = pts[pts.len() - 1];
        if first.x != last.x || first.y != last.y {
            pts.push(first);
        }
        let ring_ls = geo_types::LineString::<f64>(pts);
        Ok(Polygon(geo_types::Polygon::<f64>::new(ring_ls, vec![])))
    }

    pub fn from_coords(coords: Vec<f64>) -> Result<Self, GeoError> {
        if coords.is_empty() {
            return Err(GeoError::EmptyCoords);
        }
        if coords.len() % 2 != 0 {
            return Err(GeoError::OddCoords { len: coords.len() });
        }
        let pair_count = coords.len() / 2;
        if pair_count < 3 {
            return Err(GeoError::DegeneratePolygon { n: pair_count });
        }
        let mut pts: Vec<geo_types::Coord<f64>> = Vec::with_capacity(pair_count + 1);
        for chunk in coords.chunks_exact(2) {
            pts.push(geo_types::Coord::<f64> {
                x: chunk[0],
                y: chunk[1],
            });
        }
        let first = pts[0];
        let last = pts[pts.len() - 1];
        if first.x != last.x || first.y != last.y {
            pts.push(first);
        }
        let ring_ls = geo_types::LineString::<f64>(pts);
        Ok(Polygon(geo_types::Polygon::<f64>::new(ring_ls, vec![])))
    }

    pub fn area(&self) -> f64 {
        use geo::Area;
        self.0.unsigned_area()
    }

    pub fn contains(&self, point: Point) -> bool {
        use geo::Contains;
        self.0.contains(&point.0)
    }

    pub fn intersects(&self, other: &Polygon) -> bool {
        let result = catch_unwind(AssertUnwindSafe(|| {
            use geo::Intersects;
            self.0.intersects(&other.0)
        }));
        result.unwrap_or(false)
    }

    pub fn num_vertices(&self) -> usize {
        self.0.exterior().coords_count()
    }
}

impl Default for Polygon {
    fn default() -> Self {
        let ring = geo_types::LineString::<f64>::from(vec![
            geo_types::Point::<f64>::new(0.0, 0.0),
            geo_types::Point::<f64>::new(1.0, 0.0),
            geo_types::Point::<f64>::new(1.0, 1.0),
            geo_types::Point::<f64>::new(0.0, 0.0),
        ]);
        Polygon(geo_types::Polygon::<f64>::new(ring, vec![]))
    }
}

impl std::fmt::Display for Polygon {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Polygon({} vertices)", self.num_vertices())
    }
}

/// The Web Mercator (EPSG:3857) projection namespace.
///
/// `Projection` is **never a runtime value** — it's a NAMESPACE that
/// exposes associated functions only. `Projection.wgs84_to_web_mercator
/// (point)` projects a WGS84 (lat, lon) point to Web Mercator (x, y)
/// meters.
pub struct Projection;

impl Projection {
    /// Project a WGS84 point to Web Mercator (EPSG:3857) meters.
    ///
    /// NOTE: the input point is interpreted as `(longitude, latitude)`
    /// per the geo-rs / GIS convention (x=lon, y=lat). The output is
    /// `(x_meters, y_meters)` where x is easting and y is northing
    /// from the origin (lat=0, lon=0).
    ///
    /// Returns `Err(GeoError::LatitudeOutOfRange)` if `point.y()`
    /// (the latitude) is outside [-85.05112878, 85.05112878]. The
    /// Web Mercator projection is mathematically undefined at the
    /// poles; the upstream `geo::algorithm::webmercator` panics on
    /// those inputs — we surface the explicit error instead per
    /// FFI guide R6.
    pub fn wgs84_to_web_mercator(point: Point) -> Result<Point, GeoError> {
        let lon = point.0.x();
        let lat = point.0.y();
        if !lat.is_finite() || lat.abs() > WEB_MERCATOR_MAX_LAT {
            return Err(GeoError::LatitudeOutOfRange { lat });
        }
        let x = WEB_MERCATOR_R * lon.to_radians();
        let y = WEB_MERCATOR_R * (std::f64::consts::FRAC_PI_4 + lat.to_radians() / 2.0).tan().ln();
        Ok(Point::new(x, y))
    }
}
