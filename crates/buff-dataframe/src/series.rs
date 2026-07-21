use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ColumnKind {
    Int,
    Float,
    String,
    Bool,
}

impl ColumnKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ColumnKind::Int => "int",
            ColumnKind::Float => "float",
            ColumnKind::String => "string",
            ColumnKind::Bool => "bool",
        }
    }
}

impl fmt::Display for ColumnKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Series {
    Int(Vec<i64>),
    Float(Vec<f64>),
    String(Vec<String>),
    Bool(Vec<bool>),
}

impl Series {
    pub fn len(&self) -> usize {
        match self {
            Series::Int(v) => v.len(),
            Series::Float(v) => v.len(),
            Series::String(v) => v.len(),
            Series::Bool(v) => v.len(),
        }
    }

    pub fn kind(&self) -> ColumnKind {
        match self {
            Series::Int(_) => ColumnKind::Int,
            Series::Float(_) => ColumnKind::Float,
            Series::String(_) => ColumnKind::String,
            Series::Bool(_) => ColumnKind::Bool,
        }
    }

    pub fn as_int_slice(&self) -> Option<&[i64]> {
        match self {
            Series::Int(v) => Some(v.as_slice()),
            _ => None,
        }
    }

    pub fn as_float_slice(&self) -> Option<&[f64]> {
        match self {
            Series::Float(v) => Some(v.as_slice()),
            _ => None,
        }
    }

    pub fn as_string_slice(&self) -> Option<&[String]> {
        match self {
            Series::String(v) => Some(v.as_slice()),
            _ => None,
        }
    }

    pub fn as_bool_slice(&self) -> Option<&[bool]> {
        match self {
            Series::Bool(v) => Some(v.as_slice()),
            _ => None,
        }
    }

    pub(crate) fn fmt_cell(&self, idx: usize, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Series::Int(v) => write!(f, "{}", v.get(idx).copied().unwrap_or_default()),
            Series::Float(v) => {
                let val = v.get(idx).copied().unwrap_or_default();
                if val.fract() == 0.0 && val.is_finite() {
                    write!(f, "{val:.1}")
                } else {
                    write!(f, "{val}")
                }
            }
            Series::String(v) => match v.get(idx) {
                Some(s) => write!(f, "{s}"),
                None => f.write_str(""),
            },
            Series::Bool(v) => write!(f, "{}", v.get(idx).copied().unwrap_or(false)),
        }
    }

    pub(crate) fn slice(&self, start: usize, end: usize) -> Series {
        match self {
            Series::Int(v) => Series::Int(v[start.min(v.len())..end.min(v.len())].to_vec()),
            Series::Float(v) => Series::Float(v[start.min(v.len())..end.min(v.len())].to_vec()),
            Series::String(v) => Series::String(v[start.min(v.len())..end.min(v.len())].to_vec()),
            Series::Bool(v) => Series::Bool(v[start.min(v.len())..end.min(v.len())].to_vec()),
        }
    }

    pub(crate) fn select_indices(&self, idxs: &[usize]) -> Series {
        match self {
            Series::Int(v) => Series::Int(idxs.iter().map(|&i| v.get(i).copied().unwrap_or_default()).collect()),
            Series::Float(v) => Series::Float(idxs.iter().map(|&i| v.get(i).copied().unwrap_or_default()).collect()),
            Series::String(v) => Series::String(
                idxs.iter()
                    .map(|&i| v.get(i).cloned().unwrap_or_default())
                    .collect(),
            ),
            Series::Bool(v) => Series::Bool(idxs.iter().map(|&i| v.get(i).copied().unwrap_or_default()).collect()),
        }
    }
}

impl fmt::Display for Series {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = self.kind();
        write!(f, "{kind}[")?;
        for i in 0..self.len() {
            if i > 0 {
                f.write_str(", ")?;
            }
            self.fmt_cell(i, f)?;
        }
        f.write_str("]")
    }
}
