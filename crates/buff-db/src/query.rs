use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub struct Query {
    table: String,
    select_cols: Vec<String>,
    where_clauses: Vec<String>,
    joins: Vec<JoinSpec>,
    order_by: Vec<String>,
    limit: Option<i64>,
    offset: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
struct JoinSpec {
    kind: JoinKind,
    table: String,
    on: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinKind {
    Inner,
    Left,
    Right,
}

impl JoinKind {
    pub fn as_str(self) -> &'static str {
        match self {
            JoinKind::Inner => "INNER",
            JoinKind::Left => "LEFT",
            JoinKind::Right => "RIGHT",
        }
    }
}

impl Query {
    pub fn new(table: &str) -> Query {
        Query {
            table: table.to_string(),
            select_cols: Vec::new(),
            where_clauses: Vec::new(),
            joins: Vec::new(),
            order_by: Vec::new(),
            limit: None,
            offset: None,
        }
    }

    pub fn select(mut self, cols: &[&str]) -> Query {
        self.select_cols = cols.iter().map(|s| s.to_string()).collect();
        self
    }

    pub fn select_all(self) -> Query {
        self
    }

    pub fn filter(mut self, predicate: &str) -> Query {
        if !predicate.is_empty() {
            self.where_clauses.push(predicate.to_string());
        }
        self
    }

    pub fn join(mut self, kind: JoinKind, table: &str, on: &str) -> Query {
        self.joins.push(JoinSpec {
            kind,
            table: table.to_string(),
            on: on.to_string(),
        });
        self
    }

    pub fn inner_join(self, table: &str, on: &str) -> Query {
        self.join(JoinKind::Inner, table, on)
    }

    pub fn left_join(self, table: &str, on: &str) -> Query {
        self.join(JoinKind::Left, table, on)
    }

    pub fn order_by(mut self, col: &str) -> Query {
        if !col.is_empty() {
            self.order_by.push(col.to_string());
        }
        self
    }

    pub fn limit(mut self, n: i64) -> Query {
        if n >= 0 {
            self.limit = Some(n);
        }
        self
    }

    pub fn offset(mut self, n: i64) -> Query {
        if n >= 0 {
            self.offset = Some(n);
        }
        self
    }

    pub fn sql(&self) -> String {
        let cols = if self.select_cols.is_empty() {
            "*".to_string()
        } else {
            self.select_cols.join(", ")
        };
        let mut out = format!("SELECT {cols} FROM {}", self.table);
        for j in &self.joins {
            out.push_str(&format!(" {} JOIN {} ON {}", j.kind.as_str(), j.table, j.on));
        }
        if !self.where_clauses.is_empty() {
            out.push_str(" WHERE ");
            out.push_str(&self.where_clauses.join(" AND "));
        }
        if !self.order_by.is_empty() {
            out.push_str(" ORDER BY ");
            out.push_str(&self.order_by.join(", "));
        }
        if let Some(n) = self.limit {
            out.push_str(&format!(" LIMIT {n}"));
        }
        if let Some(n) = self.offset {
            out.push_str(&format!(" OFFSET {n}"));
        }
        out
    }
}

impl fmt::Display for Query {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.sql())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select_all_default() {
        let q = Query::new("users").sql();
        assert_eq!(q, "SELECT * FROM users");
    }

    #[test]
    fn select_cols_explicit() {
        let q = Query::new("users").select(&["id", "name"]).sql();
        assert_eq!(q, "SELECT id, name FROM users");
    }

    #[test]
    fn where_clause() {
        let q = Query::new("users").filter("age > 18").sql();
        assert_eq!(q, "SELECT * FROM users WHERE age > 18");
    }

    #[test]
    fn multiple_where_clauses_anded() {
        let q = Query::new("users")
            .filter("age > 18")
            .filter("city = 'London'")
            .sql();
        assert_eq!(
            q,
            "SELECT * FROM users WHERE age > 18 AND city = 'London'"
        );
    }

    #[test]
    fn empty_where_ignored() {
        let q = Query::new("users").filter("").sql();
        assert_eq!(q, "SELECT * FROM users");
    }

    #[test]
    fn inner_join_renders() {
        let q = Query::new("users")
            .inner_join("orders", "orders.user_id = users.id")
            .sql();
        assert_eq!(
            q,
            "SELECT * FROM users INNER JOIN orders ON orders.user_id = users.id"
        );
    }

    #[test]
    fn left_join_renders() {
        let q = Query::new("users")
            .left_join("orders", "orders.user_id = users.id")
            .sql();
        assert_eq!(
            q,
            "SELECT * FROM users LEFT JOIN orders ON orders.user_id = users.id"
        );
    }

    #[test]
    fn order_by_renders() {
        let q = Query::new("users").order_by("name").sql();
        assert_eq!(q, "SELECT * FROM users ORDER BY name");
    }

    #[test]
    fn limit_offset_renders() {
        let q = Query::new("users").limit(10).offset(20).sql();
        assert_eq!(q, "SELECT * FROM users LIMIT 10 OFFSET 20");
    }

    #[test]
    fn negative_limit_ignored() {
        let q = Query::new("users").limit(-1).sql();
        assert_eq!(q, "SELECT * FROM users");
    }

    #[test]
    fn full_pipeline() {
        let q = Query::new("users")
            .select(&["id", "name", "email"])
            .inner_join("profiles", "profiles.user_id = users.id")
            .filter("age > 18")
            .filter("active = true")
            .order_by("name")
            .limit(10)
            .sql();
        assert_eq!(
            q,
            "SELECT id, name, email FROM users \
             INNER JOIN profiles ON profiles.user_id = users.id \
             WHERE age > 18 AND active = true \
             ORDER BY name \
             LIMIT 10"
        );
    }

    #[test]
    fn acceptance_test_from_t18_spec() {
        let q = Query::new("users")
            .select(&["id", "name"])
            .filter("age > 18");
        assert_eq!(q.sql(), "SELECT id, name FROM users WHERE age > 18");
    }
}
