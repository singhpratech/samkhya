//! Synthetic-schema queries — designed to exercise multi-join
//! cardinality estimation under correlated predicates.
//!
//! Queries use `SELECT order_id` (or another small column) rather than
//! `count(*)` so the harness measures the optimizer's row-count estimate
//! against the actual unaggregated row count — i.e. the *join* or
//! *filter* cardinality, not the trivial 1-row output of an aggregate.

use super::Query;

pub const QUERIES: &[Query] = &[
    Query {
        name: "S1",
        sql: "SELECT order_id FROM orders WHERE status = 'delivered' AND amount > 3000",
    },
    Query {
        name: "S2",
        sql: "SELECT o.order_id FROM orders o JOIN customers c ON o.customer_id = c.customer_id WHERE c.region = 'US' AND o.status = 'shipped'",
    },
    Query {
        name: "S3",
        sql: "SELECT oi.order_id FROM order_items oi JOIN orders o ON oi.order_id = o.order_id WHERE o.status = 'delivered' AND oi.quantity > 5",
    },
    Query {
        name: "S4",
        sql: "SELECT oi.order_id FROM order_items oi JOIN orders o ON oi.order_id = o.order_id JOIN customers c ON o.customer_id = c.customer_id JOIN products p ON oi.product_id = p.product_id WHERE c.region = 'US' AND p.category = 'electronics' AND o.amount > 2000",
    },
    Query {
        name: "S5",
        sql: "SELECT oi.order_id FROM order_items oi JOIN orders o ON oi.order_id = o.order_id JOIN customers c ON o.customer_id = c.customer_id WHERE c.segment = 'enterprise' AND o.status = 'delivered'",
    },
];
