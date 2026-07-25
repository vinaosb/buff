use std::collections::BTreeMap;

use crate::error::{DbError, Result};

#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub columns: Vec<String>,
    pub values: Vec<DbValue>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DbValue {
    Null,
    Int(i64),
    Float(f64),
    Text(String),
    Bool(bool),
    Bytes(Vec<u8>),
}

impl DbValue {
    pub fn as_int(&self) -> Option<i64> {
        match self {
            DbValue::Int(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_float(&self) -> Option<f64> {
        match self {
            DbValue::Float(n) => Some(*n),
            DbValue::Int(n) => Some(*n as f64),
            _ => None,
        }
    }

    pub fn as_text(&self) -> Option<&str> {
        match self {
            DbValue::Text(s) => Some(s.as_str()),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            DbValue::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, DbValue::Null)
    }

    pub fn to_string_value(&self) -> String {
        match self {
            DbValue::Null => String::new(),
            DbValue::Int(n) => n.to_string(),
            DbValue::Float(n) => n.to_string(),
            DbValue::Text(s) => s.clone(),
            DbValue::Bool(b) => b.to_string(),
            DbValue::Bytes(b) => format!("0x{}", hex_bytes(b)),
        }
    }
}

fn hex_bytes(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

impl Row {
    pub fn get(&self, name: &str) -> Option<&DbValue> {
        self.columns
            .iter()
            .position(|c| c == name)
            .and_then(|i| self.values.get(i))
    }

    pub fn to_map(&self) -> BTreeMap<String, String> {
        let mut m = BTreeMap::new();
        for (i, col) in self.columns.iter().enumerate() {
            if let Some(v) = self.values.get(i) {
                m.insert(col.clone(), v.to_string_value());
            }
        }
        m
    }

    pub fn column_names(&self) -> &[String] {
        &self.columns
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

pub fn row_from_any(row: &sqlx::any::AnyRow) -> Result<Row> {
    use sqlx::Column;
    use sqlx::Row;
    let mut columns = Vec::new();
    let mut values = Vec::new();
    for (i, col) in row.columns().iter().enumerate() {
        let name = col.name().to_string();
        let val = read_any_value(row, i);
        columns.push(name);
        values.push(val);
    }
    if columns.is_empty() {
        return Err(DbError::ColumnMissing("(empty row)".into()));
    }
    Ok(crate::row::Row { columns, values })
}

fn read_any_value(row: &sqlx::any::AnyRow, i: usize) -> DbValue {
    use sqlx::Row;
    if let Ok(Some(n)) = row.try_get::<Option<i64>, _>(i) {
        return DbValue::Int(n);
    }
    if let Ok(Some(n)) = row.try_get::<Option<f64>, _>(i) {
        return DbValue::Float(n);
    }
    if let Ok(Some(s)) = row.try_get::<Option<String>, _>(i) {
        return DbValue::Text(s);
    }
    if let Ok(Some(b)) = row.try_get::<Option<bool>, _>(i) {
        return DbValue::Bool(b);
    }
    if let Ok(Some(b)) = row.try_get::<Option<Vec<u8>>, _>(i) {
        return DbValue::Bytes(b);
    }
    DbValue::Null
}
