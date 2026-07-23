use std::collections::BTreeMap;
use std::fmt;

use crate::error::{DfError, Result};
use crate::groupby::{AggOp, GroupBy};
use crate::read;
use crate::series::{ColumnKind, Series};

/// Schema-aware columnar DataFrame.
///
/// Internally a `BTreeMap` of column-name → [`Series`] plus an ordered
/// list of column names (the schema's declared order). All operations
/// are eager (no lazy execution in MVP — deferred to v1.18+ per T7 spec).
#[derive(Debug, Clone, PartialEq)]
pub struct DataFrame {
    columns: BTreeMap<String, Series>,
    order: Vec<String>,
    // `pub(crate)` so the sibling `groupby` module can stamp the
    // grouping marker via `GroupBy::into_df` (Buff's `df.group_by(col)`
    // codegen lowers to `gb.into_df()`, preserving the DataFrame
    // receiver type so subsequent `.agg(col, op)` calls dispatch on the
    // DataFrame receiver — a true GroupBy intermediate type is deferred
    // to v1.18+).
    pub(crate) grouped_by: Option<String>,
}

impl DataFrame {
    pub fn from_rows(headers: Vec<String>, rows: Vec<Vec<String>>) -> DataFrame {
        if headers.is_empty() {
            return DataFrame::empty();
        }
        let ncols = headers.len();
        let kinds: Vec<ColumnKind> = (0..ncols)
            .map(|col| infer_column_kind(rows.iter().map(|r| r.get(col).map(|s| s.as_str()))))
            .collect();
        let mut columns: BTreeMap<String, Series> = BTreeMap::new();
        for (col, name) in headers.iter().enumerate() {
            columns.insert(name.clone(), build_series(kinds[col], rows.iter(), col));
        }
        DataFrame {
            columns,
            order: headers,
            grouped_by: None,
        }
    }

    pub fn from_csv<P: AsRef<std::path::Path>>(path: P) -> Result<DataFrame> {
        read::load_csv(path)
    }

    pub fn from_json<P: AsRef<std::path::Path>>(path: P) -> Result<DataFrame> {
        read::load_json(path)
    }

    pub fn column_names(&self) -> Vec<&str> {
        self.order.iter().map(|s| s.as_str()).collect()
    }

    pub fn ncols(&self) -> usize {
        self.order.len()
    }

    pub fn len(&self) -> usize {
        self.order
            .first()
            .and_then(|name| self.columns.get(name))
            .map(|s| s.len())
            .unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn get_column(&self, name: &str) -> Option<&Series> {
        self.columns.get(name)
    }

    pub fn select(&self, cols: &[&str]) -> Result<DataFrame> {
        let mut columns: BTreeMap<String, Series> = BTreeMap::new();
        let mut order = Vec::with_capacity(cols.len());
        for &col in cols {
            let series = self
                .columns
                .get(col)
                .ok_or_else(|| DfError::UnknownColumn(col.to_string()))?;
            columns.insert(col.to_string(), series.clone());
            order.push(col.to_string());
        }
        Ok(DataFrame {
            columns,
            order,
            grouped_by: None,
        })
    }

    pub fn filter<F>(&self, predicate: F) -> Result<DataFrame>
    where
        F: Fn(&RowView<'_>) -> bool,
    {
        let mut mask: Vec<bool> = Vec::with_capacity(self.len());
        for i in 0..self.len() {
            let row = RowView { df: self, idx: i };
            mask.push(predicate(&row));
        }
        let mut columns: BTreeMap<String, Series> = BTreeMap::new();
        for name in &self.order {
            let series = match self.columns.get(name) {
                Some(s) => s,
                None => continue,
            };
            let indices: Vec<usize> = (0..series.len())
                .filter(|&i| mask.get(i).copied().unwrap_or(false))
                .collect();
            columns.insert(name.clone(), series.select_indices(&indices));
        }
        Ok(DataFrame {
            columns,
            order: self.order.clone(),
            grouped_by: None,
        })
    }

    pub fn sort(&self, col: &str) -> Result<DataFrame> {
        let series = self
            .columns
            .get(col)
            .ok_or_else(|| DfError::UnknownColumn(col.to_string()))?;
        let len = series.len();
        let mut indices: Vec<usize> = (0..len).collect();
        indices.sort_by(|&a, &b| compare_cells(series, a, b));
        let mut columns: BTreeMap<String, Series> = BTreeMap::new();
        for name in &self.order {
            let s = match self.columns.get(name) {
                Some(s) => s,
                None => continue,
            };
            columns.insert(name.clone(), s.select_indices(&indices));
        }
        Ok(DataFrame {
            columns,
            order: self.order.clone(),
            grouped_by: None,
        })
    }

    pub fn head(&self, n: usize) -> DataFrame {
        let limit = n.min(self.len());
        let mut columns: BTreeMap<String, Series> = BTreeMap::new();
        for name in &self.order {
            let s = match self.columns.get(name) {
                Some(s) => s,
                None => continue,
            };
            columns.insert(name.clone(), s.slice(0, limit));
        }
        DataFrame {
            columns,
            order: self.order.clone(),
            grouped_by: None,
        }
    }

    pub fn join(&self, other: &DataFrame, on: &str) -> Result<DataFrame> {
        let left_key = self
            .columns
            .get(on)
            .ok_or_else(|| DfError::UnknownColumn(on.to_string()))?;
        let right_key = other
            .columns
            .get(on)
            .ok_or_else(|| DfError::UnknownColumn(on.to_string()))?;
        let right_index: BTreeMap<String, usize> = build_lookup(right_key);
        let mut matched_left: Vec<usize> = Vec::new();
        let mut matched_right: Vec<usize> = Vec::new();
        for (i, key) in iterate_as_string(left_key).into_iter().enumerate() {
            if let Some(&j) = right_index.get(&key) {
                matched_left.push(i);
                matched_right.push(j);
            }
        }
        let mut order: Vec<String> = self.order.clone();
        for name in &other.order {
            if name != on && !order.contains(name) {
                order.push(name.clone());
            }
        }
        let mut columns: BTreeMap<String, Series> = BTreeMap::new();
        for name in &self.order {
            let s = match self.columns.get(name) {
                Some(s) => s,
                None => continue,
            };
            columns.insert(name.clone(), s.select_indices(&matched_left));
        }
        for name in &other.order {
            if name == on {
                continue;
            }
            let s = match other.columns.get(name) {
                Some(s) => s,
                None => continue,
            };
            columns.insert(name.clone(), s.select_indices(&matched_right));
        }
        Ok(DataFrame {
            columns,
            order,
            grouped_by: None,
        })
    }

    pub fn group_by(&self, col: &str) -> Result<GroupBy> {
        let series = self
            .columns
            .get(col)
            .ok_or_else(|| DfError::UnknownColumn(col.to_string()))?;
        let keys = iterate_as_string(series);
        if keys.is_empty() {
            return Ok(GroupBy::empty(col.to_string()));
        }
        let mut groups: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (i, k) in keys.into_iter().enumerate() {
            groups.entry(k).or_default().push(i);
        }
        Ok(GroupBy {
            key_column: col.to_string(),
            groups,
            parent: self.clone(),
        })
    }

    /// `df.agg(col, op) -> DataFrame`. If `self.grouped_by` is set
    /// (from a prior `df.group_by(...).into_df()`), performs the
    /// aggregation per group; otherwise aggregates over the entire
    /// `col` as a single group. The result DataFrame has two columns:
    /// the group key (or the literal `"all"` when no grouping is set)
    /// and the aggregated value as a String.
    pub fn agg(&self, col: &str, op: AggOp) -> DataFrame {
        match &self.grouped_by {
            Some(group_col) => {
                let gb_result = self.group_by(group_col);
                match gb_result {
                    Ok(gb) => gb.agg(col, op),
                    Err(_) => DataFrame::empty(),
                }
            }
            None => {
                let series = match self.get_column(col) {
                    Some(s) => s,
                    None => return DataFrame::empty(),
                };
                let indices: Vec<usize> = (0..series.len()).collect();
                let val = crate::groupby::aggregate(series, &indices, op);
                let key = "all".to_string();
                DataFrame::from_rows(
                    vec!["group".to_string(), col.to_string()],
                    vec![vec![key, val]],
                )
            }
        }
    }

    pub fn to_table_string(&self) -> String {
        let mut widths: Vec<usize> = self.order.iter().map(|name| name.len()).collect();
        let rows = self.len();
        for r in 0..rows {
            for (c, name) in self.order.iter().enumerate() {
                let series = match self.columns.get(name) {
                    Some(s) => s,
                    None => continue,
                };
                let mut buf = String::new();
                let _ = series.fmt_cell(r, &mut std::fmt::Formatter::new(&mut buf));
                widths[c] = widths[c].max(buf.chars().count());
            }
        }
        let mut out = String::new();
        for (c, name) in self.order.iter().enumerate() {
            if c > 0 {
                out.push_str(" | ");
            }
            pad_to(&mut out, name, widths[c]);
        }
        out.push('\n');
        for (c, _) in self.order.iter().enumerate() {
            if c > 0 {
                out.push_str("-+-");
            }
            for _ in 0..widths[c] {
                out.push('-');
            }
        }
        out.push('\n');
        for r in 0..rows {
            for (c, name) in self.order.iter().enumerate() {
                if c > 0 {
                    out.push_str(" | ");
                }
                let series = match self.columns.get(name) {
                    Some(s) => s,
                    None => continue,
                };
                let mut buf = String::new();
                let _ = series.fmt_cell(r, &mut std::fmt::Formatter::new(&mut buf));
                pad_to(&mut out, &buf, widths[c]);
            }
            out.push('\n');
        }
        out
    }

    fn empty() -> DataFrame {
        DataFrame {
            columns: BTreeMap::new(),
            order: Vec::new(),
            grouped_by: None,
        }
    }

    pub(crate) fn from_parts(columns: BTreeMap<String, Series>, order: Vec<String>) -> DataFrame {
        DataFrame {
            columns,
            order,
            grouped_by: None,
        }
    }
}

impl Default for DataFrame {
    fn default() -> Self {
        DataFrame::empty()
    }
}

impl fmt::Display for DataFrame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_table_string())
    }
}

pub struct RowView<'a> {
    df: &'a DataFrame,
    idx: usize,
}

impl<'a> RowView<'a> {
    pub fn get_int(&self, col: &str) -> Option<i64> {
        self.df
            .get_column(col)
            .and_then(|s| s.as_int_slice())
            .and_then(|v| v.get(self.idx).copied())
    }

    pub fn get_float(&self, col: &str) -> Option<f64> {
        self.df
            .get_column(col)
            .and_then(|s| s.as_float_slice())
            .and_then(|v| v.get(self.idx).copied())
    }

    pub fn get_string(&self, col: &str) -> Option<&str> {
        self.df
            .get_column(col)
            .and_then(|s| s.as_string_slice())
            .and_then(|v| v.get(self.idx).map(|s| s.as_str()))
    }

    pub fn get_bool(&self, col: &str) -> Option<bool> {
        self.df
            .get_column(col)
            .and_then(|s| s.as_bool_slice())
            .and_then(|v| v.get(self.idx).copied())
    }
}

fn pad_to(out: &mut String, s: &str, width: usize) {
    let len = s.chars().count();
    out.push_str(s);
    if len < width {
        for _ in 0..(width - len) {
            out.push(' ');
        }
    }
}

fn infer_column_kind<'a, I>(cells: I) -> ColumnKind
where
    I: Iterator<Item = Option<&'a str>>,
{
    let mut any = false;
    let mut all_int = true;
    let mut all_float = true;
    let mut all_bool = true;
    for cell in cells {
        let cell = match cell {
            Some(c) => c,
            None => continue,
        };
        if cell.is_empty() {
            continue;
        }
        any = true;
        if all_int && cell.parse::<i64>().is_err() {
            all_int = false;
        }
        if all_float && cell.parse::<f64>().is_err() {
            all_float = false;
        }
        if all_bool && !matches!(cell, "true" | "false") {
            all_bool = false;
        }
        if !(all_int || all_float || all_bool) {
            return ColumnKind::String;
        }
    }
    if !any {
        return ColumnKind::String;
    }
    if all_bool {
        ColumnKind::Bool
    } else if all_int {
        ColumnKind::Int
    } else if all_float {
        ColumnKind::Float
    } else {
        ColumnKind::String
    }
}

fn build_series<'a, I>(kind: ColumnKind, rows: I, col: usize) -> Series
where
    I: Iterator<Item = &'a Vec<String>>,
{
    match kind {
        ColumnKind::Int => {
            let v: Vec<i64> = rows
                .map(|r| {
                    r.get(col)
                        .and_then(|s| s.trim().parse::<i64>().ok())
                        .unwrap_or_default()
                })
                .collect();
            Series::Int(v)
        }
        ColumnKind::Float => {
            let v: Vec<f64> = rows
                .map(|r| {
                    r.get(col)
                        .and_then(|s| s.trim().parse::<f64>().ok())
                        .unwrap_or_default()
                })
                .collect();
            Series::Float(v)
        }
        ColumnKind::Bool => {
            let v: Vec<bool> = rows
                .map(|r| matches!(r.get(col).map(|s| s.as_str()), Some("true")))
                .collect();
            Series::Bool(v)
        }
        ColumnKind::String => {
            let v: Vec<String> = rows
                .map(|r| r.get(col).cloned().unwrap_or_default())
                .collect();
            Series::String(v)
        }
    }
}

fn compare_cells(s: &Series, a: usize, b: usize) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match s {
        Series::Int(v) => v
            .get(a)
            .copied()
            .unwrap_or_default()
            .cmp(&v.get(b).copied().unwrap_or_default()),
        Series::Float(v) => {
            let x = v.get(a).copied().unwrap_or_default();
            let y = v.get(b).copied().unwrap_or_default();
            x.partial_cmp(&y).unwrap_or(Ordering::Equal)
        }
        Series::String(v) => v
            .get(a)
            .map(|s| s.as_str())
            .unwrap_or("")
            .cmp(v.get(b).map(|s| s.as_str()).unwrap_or("")),
        Series::Bool(v) => v
            .get(a)
            .copied()
            .unwrap_or(false)
            .cmp(&v.get(b).copied().unwrap_or(false)),
    }
}

fn iterate_as_string(s: &Series) -> Vec<String> {
    match s {
        Series::Int(v) => v.iter().map(|x| x.to_string()).collect(),
        Series::Float(v) => v.iter().map(|x| x.to_string()).collect(),
        Series::String(v) => v.clone(),
        Series::Bool(v) => v.iter().map(|x| x.to_string()).collect(),
    }
}

fn build_lookup(s: &Series) -> BTreeMap<String, usize> {
    iterate_as_string(s)
        .into_iter()
        .enumerate()
        .map(|(i, k)| (k, i))
        .collect()
}
