use std::collections::BTreeMap;

use crate::dataframe::DataFrame;
use crate::series::Series;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AggOp {
    Sum,
    Mean,
    Min,
    Max,
    Count,
}

impl AggOp {
    pub fn as_str(self) -> &'static str {
        match self {
            AggOp::Sum => "sum",
            AggOp::Mean => "mean",
            AggOp::Min => "min",
            AggOp::Max => "max",
            AggOp::Count => "count",
        }
    }

    pub fn parse(s: &str) -> Option<AggOp> {
        Some(match s {
            "sum" => AggOp::Sum,
            "mean" | "avg" => AggOp::Mean,
            "min" => AggOp::Min,
            "max" => AggOp::Max,
            "count" => AggOp::Count,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct GroupBy {
    pub(crate) key_column: String,
    pub(crate) groups: BTreeMap<String, Vec<usize>>,
    pub(crate) parent: DataFrame,
}

impl GroupBy {
    pub fn agg(&self, col: &str, op: AggOp) -> DataFrame {
        let series = match self.parent.get_column(col) {
            Some(s) => s,
            None => {
                let mut columns = BTreeMap::new();
                columns.insert(self.key_column.clone(), Series::String(Vec::new()));
                columns.insert(col.to_string(), Series::String(Vec::new()));
                return DataFrame::from_parts(
                    columns,
                    vec![self.key_column.clone(), col.to_string()],
                );
            }
        };
        let mut key_col: Vec<String> = Vec::with_capacity(self.groups.len());
        let mut val_col: Vec<String> = Vec::with_capacity(self.groups.len());
        for (key, indices) in &self.groups {
            key_col.push(key.clone());
            let val = aggregate(series, indices, op);
            val_col.push(val);
        }
        // Build with explicit String series: routing through
        // `DataFrame::from_rows` would re-infer numeric-looking sums as
        // Int/Float, defeating the documented String-coerced aggregation
        // contract (users re-coerce via `as_int_slice()` etc).
        let mut columns = BTreeMap::new();
        columns.insert(self.key_column.clone(), Series::String(key_col));
        columns.insert(col.to_string(), Series::String(val_col));
        DataFrame::from_parts(columns, vec![self.key_column.clone(), col.to_string()])
    }

    pub fn len(&self) -> usize {
        self.groups.len()
    }

    pub fn into_df(mut self) -> DataFrame {
        self.parent.grouped_by = Some(self.key_column.clone());
        self.parent
    }

    pub(crate) fn empty(key_column: String) -> GroupBy {
        GroupBy {
            key_column,
            groups: BTreeMap::new(),
            parent: DataFrame::from_rows(Vec::new(), Vec::new()),
        }
    }
}

pub(crate) fn aggregate(series: &Series, indices: &[usize], op: AggOp) -> String {
    match op {
        AggOp::Count => return indices.len().to_string(),
        _ => {}
    }
    match series {
        Series::Int(v) => {
            let xs: Vec<i64> = indices
                .iter()
                .map(|&i| v.get(i).copied().unwrap_or_default())
                .collect();
            match op {
                AggOp::Sum => xs.iter().sum::<i64>().to_string(),
                AggOp::Mean => {
                    if xs.is_empty() {
                        "0".to_string()
                    } else {
                        format!("{}", (xs.iter().sum::<i64>() as f64) / xs.len() as f64)
                    }
                }
                AggOp::Min => xs.iter().min().copied().unwrap_or_default().to_string(),
                AggOp::Max => xs.iter().max().copied().unwrap_or_default().to_string(),
                // Count is handled by the early return above; this arm is a
                // defensive fallback that mirrors it (count is type-agnostic).
                AggOp::Count => indices.len().to_string(),
            }
        }
        Series::Float(v) => {
            let xs: Vec<f64> = indices
                .iter()
                .map(|&i| v.get(i).copied().unwrap_or_default())
                .collect();
            match op {
                AggOp::Sum => format!("{}", xs.iter().sum::<f64>()),
                AggOp::Mean => {
                    if xs.is_empty() {
                        "0".to_string()
                    } else {
                        format!("{}", xs.iter().sum::<f64>() / xs.len() as f64)
                    }
                }
                AggOp::Min => format!("{}", xs.iter().copied().fold(f64::INFINITY, f64::min)),
                AggOp::Max => format!("{}", xs.iter().copied().fold(f64::NEG_INFINITY, f64::max)),
                // Count is handled by the early return above; this arm is a
                // defensive fallback that mirrors it (count is type-agnostic).
                AggOp::Count => indices.len().to_string(),
            }
        }
        Series::Bool(v) => {
            let xs: Vec<bool> = indices
                .iter()
                .map(|&i| v.get(i).copied().unwrap_or_default())
                .collect();
            match op {
                AggOp::Sum => xs.iter().filter(|&&b| b).count().to_string(),
                AggOp::Mean => format!(
                    "{}",
                    xs.iter().filter(|&&b| b).count() as f64 / xs.len().max(1) as f64
                ),
                AggOp::Min => xs
                    .iter()
                    .any(|&b| !b)
                    .then_some("false")
                    .unwrap_or("true")
                    .to_string(),
                AggOp::Max => xs
                    .iter()
                    .any(|&b| b)
                    .then_some("true")
                    .unwrap_or("false")
                    .to_string(),
                // Count is handled by the early return above; this arm is a
                // defensive fallback that mirrors it (count is type-agnostic).
                AggOp::Count => indices.len().to_string(),
            }
        }
        Series::String(v) => {
            let xs: Vec<&str> = indices
                .iter()
                .map(|&i| v.get(i).map(|s| s.as_str()).unwrap_or(""))
                .collect();
            match op {
                AggOp::Min => xs.iter().min().copied().unwrap_or("").to_string(),
                AggOp::Max => xs.iter().max().copied().unwrap_or("").to_string(),
                // Count is handled by the early return above; this arm is a
                // defensive fallback that mirrors it (count is type-agnostic).
                AggOp::Count => indices.len().to_string(),
                _ => String::new(),
            }
        }
    }
}
