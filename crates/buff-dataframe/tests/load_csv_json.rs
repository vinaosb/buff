use std::io::Write;
use std::path::PathBuf;

use buff_dataframe::{ColumnKind, DataFrame};

fn write_temp(name: &str, content: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!("buff-dataframe-test-{name}-{}", std::process::id()));
    let mut file = std::fs::File::create(&path).expect("temp file");
    file.write_all(content.as_bytes()).expect("write");
    path
}

#[test]
fn load_csv_basic() {
    let csv = "name,age,city\nAda,36,London\nAlan,41,London\nGrace,85,New York\n";
    let path = write_temp("basic.csv", csv);
    let df = DataFrame::from_csv(&path).expect("load");
    assert_eq!(df.column_names(), vec!["name", "age", "city"]);
    assert_eq!(df.len(), 3);
    assert!(matches!(df.get_column("age").unwrap().kind(), ColumnKind::Int));
    assert!(matches!(df.get_column("name").unwrap().kind(), ColumnKind::String));
    let ages = df.get_column("age").unwrap().as_int_slice().unwrap();
    assert_eq!(ages, &[36, 41, 85]);
}

#[test]
fn load_csv_with_floats() {
    let csv = "x,y\n1.5,10\n2.5,20\n3.5,30\n";
    let path = write_temp("floats.csv", csv);
    let df = DataFrame::from_csv(&path).expect("load");
    assert!(matches!(df.get_column("x").unwrap().kind(), ColumnKind::Float));
    assert!(matches!(df.get_column("y").unwrap().kind(), ColumnKind::Int));
}

#[test]
fn load_csv_with_bools() {
    let csv = "flag\ntrue\nfalse\ntrue\n";
    let path = write_temp("bools.csv", csv);
    let df = DataFrame::from_csv(&path).expect("load");
    assert!(matches!(df.get_column("flag").unwrap().kind(), ColumnKind::Bool));
    assert_eq!(df.get_column("flag").unwrap().as_bool_slice().unwrap(), &[true, false, true]);
}

#[test]
fn load_csv_empty() {
    let csv = "a,b\n";
    let path = write_temp("empty.csv", csv);
    let df = DataFrame::from_csv(&path).expect("load");
    assert_eq!(df.column_names(), vec!["a", "b"]);
    assert_eq!(df.len(), 0);
}

#[test]
fn load_csv_quoted_field_with_comma() {
    let csv = "name,note\n\"Hello, World\",1\n";
    let path = write_temp("quoted.csv", csv);
    let df = DataFrame::from_csv(&path).expect("load");
    assert_eq!(df.len(), 1);
    assert_eq!(
        df.get_column("name").unwrap().as_string_slice().unwrap(),
        &["Hello, World".to_string()]
    );
}

#[test]
fn load_json_lines() {
    let json = concat!(
        "{\"name\": \"Ada\", \"age\": 36, \"active\": true}\n",
        "{\"name\": \"Alan\", \"age\": 41, \"active\": false}\n",
        "{\"name\": \"Grace\", \"age\": 85, \"active\": true}\n"
    );
    let path = write_temp("rows.jsonl", json);
    let df = DataFrame::from_json(&path).expect("load");
    assert_eq!(df.len(), 3);
    assert!(df.column_names().contains(&"name".to_string()));
    assert!(df.column_names().contains(&"age".to_string()));
    assert!(df.column_names().contains(&"active".to_string()));
    assert!(matches!(df.get_column("age").unwrap().kind(), ColumnKind::Int));
    assert!(matches!(df.get_column("active").unwrap().kind(), ColumnKind::Bool));
    assert!(matches!(df.get_column("name").unwrap().kind(), ColumnKind::String));
}

#[test]
fn load_json_floats_promote_mixed() {
    let json = concat!(
        "{\"x\": 1}\n",
        "{\"x\": 1.5}\n",
        "{\"x\": 2}\n"
    );
    let path = write_temp("mixed.jsonl", json);
    let df = DataFrame::from_json(&path).expect("load");
    assert!(matches!(df.get_column("x").unwrap().kind(), ColumnKind::Float));
}

#[test]
fn load_csv_missing_file_errors() {
    let path = std::env::temp_dir().join("definitely-not-here-139485.csv");
    let result = DataFrame::from_csv(&path);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(format!("{err}").to_lowercase().contains("io"));
}

#[test]
fn load_json_missing_file_errors() {
    let path = std::env::temp_dir().join("definitely-not-here-941374.json");
    let result = DataFrame::from_json(&path);
    assert!(result.is_err());
}

#[test]
fn load_then_select_then_head_pipeline() {
    let csv = "id,name\n1,Ada\n2,Alan\n3,Grace\n4,Linus\n5,Dennis\n6,Guido\n";
    let path = write_temp("pipeline.csv", csv);
    let df = DataFrame::from_csv(&path).expect("load");
    let top3 = df.select(&["name"]).unwrap().head(3);
    assert_eq!(top3.len(), 3);
    assert_eq!(top3.ncols(), 1);
    assert_eq!(
        top3.get_column("name").unwrap().as_string_slice().unwrap(),
        &["Ada".to_string(), "Alan".to_string(), "Grace".to_string()]
    );
}
