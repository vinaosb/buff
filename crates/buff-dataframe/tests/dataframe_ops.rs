use buff_dataframe::{AggOp, ColumnKind, DataFrame, DfError, Series};

fn fixture_users() -> DataFrame {
    DataFrame::from_rows(
        vec!["id".into(), "name".into(), "age".into(), "city".into()],
        vec![
            vec!["1".into(), "Ada".into(),    "36".into(), "London".into()],
            vec!["2".into(), "Alan".into(),   "41".into(), "London".into()],
            vec!["3".into(), "Grace".into(),  "85".into(), "New York".into()],
            vec!["4".into(), "Linus".into(),  "55".into(), "Helsinki".into()],
            vec!["5".into(), "Dennis".into(), "70".into(), "New York".into()],
        ],
    )
}

#[test]
fn from_rows_basic_inference() {
    let df = fixture_users();
    assert_eq!(df.column_names(), vec!["id", "name", "age", "city"]);
    assert_eq!(df.ncols(), 4);
    assert_eq!(df.len(), 5);
    assert!(!df.is_empty());
    assert!(matches!(df.get_column("id").unwrap().kind(), ColumnKind::Int));
    assert!(matches!(df.get_column("name").unwrap().kind(), ColumnKind::String));
    assert!(matches!(df.get_column("age").unwrap().kind(), ColumnKind::Int));
    assert!(matches!(df.get_column("city").unwrap().kind(), ColumnKind::String));
}

#[test]
fn from_rows_float_inference() {
    let df = DataFrame::from_rows(
        vec!["x".into()],
        vec![
            vec!["1.5".into()],
            vec!["2.0".into()],
            vec!["3.14".into()],
        ],
    );
    assert!(matches!(df.get_column("x").unwrap().kind(), ColumnKind::Float));
    let slice = df.get_column("x").unwrap().as_float_slice().unwrap();
    assert_eq!(slice, &[1.5, 2.0, 3.14]);
}

#[test]
fn from_rows_bool_inference() {
    let df = DataFrame::from_rows(
        vec!["flag".into()],
        vec![vec!["true".into()], vec!["false".into()], vec!["true".into()]],
    );
    assert!(matches!(df.get_column("flag").unwrap().kind(), ColumnKind::Bool));
    assert_eq!(df.get_column("flag").unwrap().as_bool_slice().unwrap(), &[true, false, true]);
}

#[test]
fn from_rows_empty_cells_kept_as_string() {
    let df = DataFrame::from_rows(
        vec!["a".into(), "b".into()],
        vec![vec!["1".into(), String::new()], vec!["x".into(), "y".into()]],
    );
    assert!(matches!(df.get_column("a").unwrap().kind(), ColumnKind::String));
    assert!(matches!(df.get_column("b").unwrap().kind(), ColumnKind::String));
}

#[test]
fn select_projection() {
    let df = fixture_users();
    let projected = df.select(&["name", "city"]).unwrap();
    assert_eq!(projected.column_names(), vec!["name", "city"]);
    assert_eq!(projected.len(), 5);
}

#[test]
fn select_unknown_column_errors() {
    let df = fixture_users();
    let err = df.select(&["nope"]).unwrap_err();
    assert!(matches!(err, DfError::UnknownColumn(_)));
}

#[test]
fn filter_by_predicate() {
    let df = fixture_users();
    let londoners = df
        .filter(|r| r.get_string("city") == Some("London"))
        .unwrap();
    assert_eq!(londoners.len(), 2);
    assert_eq!(
        londoners.get_column("name").unwrap().as_string_slice().unwrap(),
        &["Ada".to_string(), "Alan".to_string()]
    );
}

#[test]
fn filter_by_numeric_predicate() {
    let df = fixture_users();
    let seniors = df.filter(|r| r.get_int("age").unwrap_or(0) >= 60).unwrap();
    assert_eq!(seniors.len(), 3);
}

#[test]
fn sort_ascending() {
    let df = fixture_users();
    let sorted = df.sort("age").unwrap();
    let ages = sorted.get_column("age").unwrap().as_int_slice().unwrap();
    assert_eq!(ages, &[36, 41, 55, 70, 85]);
    let names = sorted.get_column("name").unwrap().as_string_slice().unwrap();
    assert_eq!(names, &["Ada", "Alan", "Linus", "Dennis", "Grace"]);
}

#[test]
fn head_truncates() {
    let df = fixture_users();
    let top3 = df.head(3);
    assert_eq!(top3.len(), 3);
    assert_eq!(
        top3.get_column("name").unwrap().as_string_slice().unwrap(),
        &["Ada".to_string(), "Alan".to_string(), "Grace".to_string()]
    );
}

#[test]
fn head_more_than_len_is_idempotent() {
    let df = fixture_users();
    assert_eq!(df.head(100).len(), 5);
}

#[test]
fn head_zero_yields_empty() {
    let df = fixture_users();
    let empty = df.head(0);
    assert!(empty.is_empty());
}

#[test]
fn group_by_count() {
    let df = fixture_users();
    let gb = df.group_by("city").unwrap();
    assert_eq!(gb.len(), 3);
}

#[test]
fn group_by_agg_mean_int() {
    let df = fixture_users();
    let agg = df.group_by("city").unwrap().agg("age", AggOp::Mean);
    assert_eq!(agg.len(), 3);
    let cities = agg.get_column("city").unwrap().as_string_slice().unwrap();
    assert!(cities.contains(&"London".to_string()));
    assert!(cities.contains(&"New York".to_string()));
    assert!(cities.contains(&"Helsinki".to_string()));
}

#[test]
fn group_by_agg_sum() {
    let df = fixture_users();
    let agg = df.group_by("city").unwrap().agg("age", AggOp::Sum);
    let cities = agg.get_column("city").unwrap().as_string_slice().unwrap();
    let sums = agg.get_column("age").unwrap().as_string_slice().unwrap();
    let london_idx = cities.iter().position(|c| c == "London").unwrap();
    assert_eq!(sums[london_idx], "77");
}

#[test]
fn join_inner_equi() {
    let users = DataFrame::from_rows(
        vec!["user_id".into(), "name".into()],
        vec![
            vec!["1".into(), "Ada".into()],
            vec!["2".into(), "Alan".into()],
            vec!["3".into(), "Grace".into()],
        ],
    );
    let orders = DataFrame::from_rows(
        vec!["user_id".into(), "total".into()],
        vec![
            vec!["1".into(), "99.50".into()],
            vec!["3".into(), "12.00".into()],
            vec!["9".into(), "0.99".into()],
        ],
    );
    let joined = users.join(&orders, "user_id").unwrap();
    assert_eq!(joined.len(), 2);
    assert!(joined.column_names().contains(&"name"));
    assert!(joined.column_names().contains(&"total"));
}

#[test]
fn to_table_string_contains_headers_and_a_row() {
    let df = fixture_users();
    let table = df.to_table_string();
    assert!(table.contains("name"));
    assert!(table.contains("Ada"));
    assert!(table.contains("London"));
}

#[test]
fn empty_dataframe_handles() {
    let df = DataFrame::from_rows(Vec::new(), Vec::new());
    assert!(df.is_empty());
    assert_eq!(df.len(), 0);
    assert_eq!(df.ncols(), 0);
    assert_eq!(df.head(5).len(), 0);
    let table = df.to_table_string();
    assert!(table.is_empty() || table.trim().is_empty());
}

#[test]
fn series_accessors() {
    let df = fixture_users();
    let id = df.get_column("id").unwrap();
    assert_eq!(id.as_int_slice().unwrap(), &[1, 2, 3, 4, 5]);
    assert!(id.as_float_slice().is_none());
    assert!(id.as_string_slice().is_none());
    assert!(id.as_bool_slice().is_none());
}

#[test]
fn agg_op_parse_round_trip() {
    use AggOp::*;
    assert_eq!(AggOp::parse("sum"), Some(Sum));
    assert_eq!(AggOp::parse("mean"), Some(Mean));
    assert_eq!(AggOp::parse("avg"), Some(Mean));
    assert_eq!(AggOp::parse("count"), Some(Count));
    assert_eq!(AggOp::parse("nope"), None);
}

#[test]
fn display_dataframe_matches_to_table_string() {
    let df = fixture_users().head(2);
    let s = format!("{df}");
    assert!(s.contains("Ada"));
}
