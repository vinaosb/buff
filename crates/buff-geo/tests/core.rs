//! Integration tests for the `buff-geo` crate (T45).
//!
//! Covers all 13 public functions per the T45 spec:
//! - Point: new, x, y, distance_to, buffer
//! - LineString: new, from_coords, length, num_points
//! - Polygon: new, from_coords, area, contains, intersects, num_vertices
//! - Projection: wgs84_to_web_mercator
//!
//! Per T45 acceptance: "Distance/area calculations correct. Intersection
//! detection works. 3 examples + 12 tests." — we ship 16 unit tests +
//! 5 insta snapshots (above the floor).

use buff_geo::{GeoError, LineString, Point, Polygon, Projection};

fn approx_eq(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() < tol
}

#[test]
fn point_new_round_trips_xy() {
    let p = Point::new(3.0, -4.5);
    assert_eq!(p.x(), 3.0);
    assert_eq!(p.y(), -4.5);
}

#[test]
fn point_distance_to_is_euclidean() {
    let a = Point::new(0.0, 0.0);
    let b = Point::new(3.0, 4.0);
    assert!(approx_eq(a.distance_to(b), 5.0, 1e-9));
    assert!(approx_eq(b.distance_to(a), 5.0, 1e-9));
    assert!(approx_eq(a.distance_to(a), 0.0, 1e-9));
}

#[test]
fn point_buffer_zero_radius_is_degenerate_polygon() {
    let p = Point::new(1.0, 2.0);
    let poly = p.buffer(0.0).expect("zero-radius buffer");
    assert!(poly.area() < 1e-12);
}

#[test]
fn point_buffer_positive_radius_produces_circle_area() {
    let p = Point::new(0.0, 0.0);
    let poly = p.buffer(5.0).expect("radius=5 buffer");
    let expected = std::f64::consts::PI * 25.0;
    assert!(
        approx_eq(poly.area(), expected, 0.5),
        "expected ~{} got {}",
        expected,
        poly.area()
    );
}

#[test]
fn point_buffer_rejects_invalid_radius() {
    let p = Point::new(0.0, 0.0);
    assert!(matches!(
        p.buffer(-1.0),
        Err(GeoError::DegeneratePolygon { .. })
    ));
    assert!(p.buffer(f64::NAN).is_err());
}

#[test]
fn line_string_new_constructs_polyline() {
    let ls = LineString::new(vec![
        Point::new(0.0, 0.0),
        Point::new(3.0, 0.0),
        Point::new(3.0, 4.0),
    ])
    .expect("construct");
    assert_eq!(ls.num_points(), 3);
    assert!(approx_eq(ls.length(), 7.0, 1e-9));
}

#[test]
fn line_string_new_rejects_empty_input() {
    let err = LineString::new(vec![]).unwrap_err();
    assert!(matches!(err, GeoError::EmptyCoords));
}

#[test]
fn line_string_from_coords_flat_pairs() {
    let ls = LineString::from_coords(vec![0.0, 0.0, 1.0, 0.0, 1.0, 1.0]).expect("construct");
    assert_eq!(ls.num_points(), 3);
    assert!(approx_eq(ls.length(), 2.0, 1e-9));
}

#[test]
fn line_string_from_coords_rejects_odd_input() {
    let err = LineString::from_coords(vec![0.0, 0.0, 1.0]).unwrap_err();
    assert!(matches!(err, GeoError::OddCoords { len: 3 }));
}

#[test]
fn polygon_new_unit_square_area_is_one() {
    let poly = Polygon::new(vec![
        Point::new(0.0, 0.0),
        Point::new(1.0, 0.0),
        Point::new(1.0, 1.0),
        Point::new(0.0, 1.0),
    ])
    .expect("construct");
    assert!(approx_eq(poly.area(), 1.0, 1e-9));
}

#[test]
fn polygon_new_4x5_rectangle_area() {
    let poly = Polygon::new(vec![
        Point::new(0.0, 0.0),
        Point::new(4.0, 0.0),
        Point::new(4.0, 5.0),
        Point::new(0.0, 5.0),
    ])
    .expect("construct");
    assert!(approx_eq(poly.area(), 20.0, 1e-9));
}

#[test]
fn polygon_new_auto_closes_open_ring() {
    let poly = Polygon::new(vec![
        Point::new(0.0, 0.0),
        Point::new(2.0, 0.0),
        Point::new(1.0, 2.0),
    ])
    .expect("construct");
    assert!(poly.num_vertices() >= 4);
    assert!(approx_eq(poly.area(), 2.0, 1e-9));
}

#[test]
fn polygon_new_rejects_degenerate_input() {
    let err = Polygon::new(vec![Point::new(0.0, 0.0), Point::new(1.0, 1.0)]).unwrap_err();
    assert!(matches!(err, GeoError::DegeneratePolygon { n: 2 }));
}

#[test]
fn polygon_from_coords_flat_pairs() {
    let poly =
        Polygon::from_coords(vec![0.0, 0.0, 4.0, 0.0, 4.0, 4.0, 0.0, 4.0]).expect("construct");
    assert!(approx_eq(poly.area(), 16.0, 1e-9));
}

#[test]
fn polygon_contains_interior_point() {
    let poly = Polygon::new(vec![
        Point::new(0.0, 0.0),
        Point::new(10.0, 0.0),
        Point::new(10.0, 10.0),
        Point::new(0.0, 10.0),
    ])
    .expect("construct");
    assert!(poly.contains(Point::new(5.0, 5.0)));
    assert!(!poly.contains(Point::new(20.0, 20.0)));
    assert!(!poly.contains(Point::new(-1.0, -1.0)));
}

#[test]
fn polygon_intersects_overlapping_squares() {
    let a = Polygon::new(vec![
        Point::new(0.0, 0.0),
        Point::new(2.0, 0.0),
        Point::new(2.0, 2.0),
        Point::new(0.0, 2.0),
    ])
    .expect("a");
    let b = Polygon::new(vec![
        Point::new(1.0, 1.0),
        Point::new(3.0, 1.0),
        Point::new(3.0, 3.0),
        Point::new(1.0, 3.0),
    ])
    .expect("b");
    assert!(a.intersects(&b));
    assert!(b.intersects(&a));
}

#[test]
fn polygon_intersects_disjoint_squares_returns_false() {
    let a = Polygon::new(vec![
        Point::new(0.0, 0.0),
        Point::new(1.0, 0.0),
        Point::new(1.0, 1.0),
        Point::new(0.0, 1.0),
    ])
    .expect("a");
    let b = Polygon::new(vec![
        Point::new(10.0, 10.0),
        Point::new(11.0, 10.0),
        Point::new(11.0, 11.0),
        Point::new(10.0, 11.0),
    ])
    .expect("b");
    assert!(!a.intersects(&b));
}

#[test]
fn projection_wgs84_to_web_mercator_origin_is_zero() {
    let origin = Point::new(0.0, 0.0);
    let projected = Projection::wgs84_to_web_mercator(origin).expect("origin projects");
    assert!(approx_eq(projected.x(), 0.0, 1e-6));
    assert!(approx_eq(projected.y(), 0.0, 1e-6));
}

#[test]
fn projection_wgs84_to_web_mercator_known_value() {
    let lon = 0.0;
    let lat = std::f64::consts::FRAC_PI_4.to_degrees();
    let p = Point::new(lon, lat);
    let projected = Projection::wgs84_to_web_mercator(p).expect("projects");
    let expected_x = 0.0;
    let expected_y = 6378137.0
        * (std::f64::consts::FRAC_PI_4 + lat.to_radians() / 2.0)
            .tan()
            .ln();
    assert!(approx_eq(projected.x(), expected_x, 1e-3));
    assert!(approx_eq(projected.y(), expected_y, 1e-3));
}

#[test]
fn projection_rejects_latitude_out_of_range() {
    let p = Point::new(0.0, 89.0);
    assert!(matches!(
        Projection::wgs84_to_web_mercator(p),
        Err(GeoError::LatitudeOutOfRange { .. })
    ));
    let q = Point::new(0.0, -89.0);
    assert!(matches!(
        Projection::wgs84_to_web_mercator(q),
        Err(GeoError::LatitudeOutOfRange { .. })
    ));
}

#[test]
fn point_default_is_origin() {
    let p = Point::default();
    assert_eq!(p.x(), 0.0);
    assert_eq!(p.y(), 0.0);
}

// ---- Insta snapshots (5+) ---------------------------------------------------

#[test]
fn snapshot_point_display() {
    let p = Point::new(1.5, -2.5);
    insta::assert_snapshot!("point_display", format!("{p}"));
}

#[test]
fn snapshot_line_string_display() {
    let ls = LineString::new(vec![
        Point::new(0.0, 0.0),
        Point::new(1.0, 0.0),
        Point::new(1.0, 1.0),
    ])
    .expect("snapshot construct");
    insta::assert_snapshot!("line_string_display", format!("{ls}"));
}

#[test]
fn snapshot_polygon_display() {
    let poly = Polygon::new(vec![
        Point::new(0.0, 0.0),
        Point::new(2.0, 0.0),
        Point::new(2.0, 2.0),
        Point::new(0.0, 2.0),
    ])
    .expect("snapshot construct");
    insta::assert_snapshot!("polygon_display", format!("{poly}"));
}

#[test]
fn snapshot_geo_error_all_variants() {
    let e1 = GeoError::EmptyCoords;
    let e2 = GeoError::OddCoords { len: 7 };
    let e3 = GeoError::DegeneratePolygon { n: 1 };
    let e4 = GeoError::LatitudeOutOfRange { lat: 89.5 };
    let e5 = GeoError::Panic;
    insta::assert_snapshot!("geo_error_all", format!("{e1}\n{e2}\n{e3}\n{e4}\n{e5}"));
}

#[test]
fn snapshot_polygon_area_known_shape() {
    let poly = Polygon::new(vec![
        Point::new(0.0, 0.0),
        Point::new(10.0, 0.0),
        Point::new(10.0, 10.0),
        Point::new(0.0, 10.0),
    ])
    .expect("snapshot construct");
    insta::assert_snapshot!("polygon_area_10x10", format!("area={:.4}", poly.area()));
}
