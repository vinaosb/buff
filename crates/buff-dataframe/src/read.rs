use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

use crate::dataframe::DataFrame;
use crate::error::{DfError, Result};
use crate::series::{ColumnKind, Series};

pub fn load_csv<P: AsRef<Path>>(path: P) -> Result<DataFrame> {
    let path_ref = path.as_ref();
    let file = File::open(path_ref)?;
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_reader(BufReader::new(file));
    let mut records: Vec<Vec<String>> = Vec::new();
    for result in reader.records() {
        let record = result?;
        records.push(record.iter().map(|s| s.to_string()).collect());
    }
    if records.is_empty() {
        return Ok(DataFrame::from_rows(Vec::new(), Vec::new()));
    }
    let header = records.remove(0);
    Ok(DataFrame::from_rows(header, records))
}

pub fn load_json<P: AsRef<Path>>(path: P) -> Result<DataFrame> {
    let mut content = String::new();
    File::open(path.as_ref())?.read_to_string(&mut content)?;
    let mut rows: Vec<BTreeMap<String, serde_json::Value>> = Vec::new();
    let mut columns_seen: BTreeMap<String, ColumnKind> = BTreeMap::new();
    let mut order: Vec<String> = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let value: serde_json::Value = serde_json::from_str(trimmed)?;
        if let serde_json::Value::Object(map) = value {
            let mut row: BTreeMap<String, serde_json::Value> = BTreeMap::new();
            for (k, v) in map {
                if !order.contains(&k) {
                    order.push(k.clone());
                }
                let kind = json_kind(&v);
                columns_seen
                    .entry(k.clone())
                    .and_modify(|existing| {
                        *existing = unify_kind(*existing, kind);
                    })
                    .or_insert(kind);
                row.insert(k, v);
            }
            rows.push(row);
        }
    }
    if order.is_empty() {
        return Ok(DataFrame::from_rows(Vec::new(), Vec::new()));
    }
    let mut columns: BTreeMap<String, Series> = BTreeMap::new();
    for name in &order {
        let kind = columns_seen
            .get(name)
            .copied()
            .unwrap_or(ColumnKind::String);
        let series = build_json_series(kind, name, &rows);
        columns.insert(name.clone(), series);
    }
    Ok(DataFrame::from_parts(columns, order))
}

fn json_kind(v: &serde_json::Value) -> ColumnKind {
    match v {
        serde_json::Value::Bool(_) => ColumnKind::Bool,
        serde_json::Value::Number(n) if n.is_i64() => ColumnKind::Int,
        serde_json::Value::Number(_) => ColumnKind::Float,
        serde_json::Value::String(_) => ColumnKind::String,
        _ => ColumnKind::String,
    }
}

fn unify_kind(a: ColumnKind, b: ColumnKind) -> ColumnKind {
    use ColumnKind::*;
    match (a, b) {
        (Int, Int) => Int,
        (Int, Float) | (Float, Int) | (Float, Float) => Float,
        (Bool, Bool) => Bool,
        _ => String,
    }
}

fn build_json_series(
    kind: ColumnKind,
    name: &str,
    rows: &[BTreeMap<String, serde_json::Value>],
) -> Series {
    match kind {
        ColumnKind::Int => {
            let v: Vec<i64> = rows
                .iter()
                .map(|r| match r.get(name) {
                    Some(serde_json::Value::Number(n)) => n.as_i64().unwrap_or_default(),
                    _ => 0,
                })
                .collect();
            Series::Int(v)
        }
        ColumnKind::Float => {
            let v: Vec<f64> = rows
                .iter()
                .map(|r| match r.get(name) {
                    Some(serde_json::Value::Number(n)) => n.as_f64().unwrap_or_default(),
                    _ => 0.0,
                })
                .collect();
            Series::Float(v)
        }
        ColumnKind::Bool => {
            let v: Vec<bool> = rows
                .iter()
                .map(|r| matches!(r.get(name), Some(serde_json::Value::Bool(true))))
                .collect();
            Series::Bool(v)
        }
        ColumnKind::String => {
            let v: Vec<String> = rows
                .iter()
                .map(|r| match r.get(name) {
                    Some(serde_json::Value::String(s)) => s.clone(),
                    Some(serde_json::Value::Number(n)) => n.to_string(),
                    Some(serde_json::Value::Bool(b)) => b.to_string(),
                    _ => String::new(),
                })
                .collect();
            Series::String(v)
        }
    }
}
