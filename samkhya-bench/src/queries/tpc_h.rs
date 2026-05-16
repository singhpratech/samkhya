//! TPC-H — placeholder entries for the five queries we will exercise first.
//!
//! Q1, Q5, Q9, Q17 and Q21 are the canonical stressors for join ordering and
//! group-by cardinality. SQL text is intentionally left as a placeholder
//! comment; the real text will be wired in once the DataFusion adapter lands.

use super::Query;

pub const QUERIES: &[Query] = &[
    Query {
        name: "Q1",
        sql: "-- TPC-H Q1: pricing summary report (placeholder)",
    },
    Query {
        name: "Q5",
        sql: "-- TPC-H Q5: local supplier volume (placeholder)",
    },
    Query {
        name: "Q9",
        sql: "-- TPC-H Q9: product type profit measure (placeholder)",
    },
    Query {
        name: "Q17",
        sql: "-- TPC-H Q17: small-quantity-order revenue (placeholder)",
    },
    Query {
        name: "Q21",
        sql: "-- TPC-H Q21: suppliers who kept orders waiting (placeholder)",
    },
];
