//! STATS-CEB — placeholder corpus.
//!
//! The STATS-CEB benchmark (Han et al., 2021) targets cardinality estimation
//! on the StackExchange schema. Queries will be populated once the schema
//! loader is in place.

use super::Query;

pub const QUERIES: &[Query] = &[Query {
    name: "stats-ceb-placeholder",
    sql: "-- STATS-CEB queries will be populated here",
}];
