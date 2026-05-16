//! H05 EXPLAIN capture — prints EXPLAIN output for each entry in the
//! fortress integration matrix so the receipt has truncated excerpts to
//! cite. Mirrors the queries in `tests/h05_fortress.rs`.

use std::sync::Arc;

use datafusion::arrow::array::{Array, Int32Array, Int64Array, StringArray};
use datafusion::arrow::datatypes::{DataType, Field, Schema};
use datafusion::arrow::record_batch::RecordBatch;
use datafusion::datasource::{MemTable, TableProvider};
use datafusion::execution::context::SessionContext;
use datafusion::execution::session_state::SessionStateBuilder;
use datafusion::prelude::SessionConfig;
use samkhya_core::stats::ColumnStats;
use samkhya_datafusion::{SamkhyaOptimizerRule, SamkhyaTableProvider};

fn samkhya_ctx() -> SessionContext {
    let rule = Arc::new(SamkhyaOptimizerRule::new());
    let state = SessionStateBuilder::new()
        .with_config(SessionConfig::new())
        .with_default_features()
        .with_optimizer_rule(rule.clone())
        .with_physical_optimizer_rule(rule.clone())
        .build();
    SessionContext::new_with_state(state)
}

fn wrap(inner: Arc<dyn TableProvider>, rows: u64) -> Arc<SamkhyaTableProvider> {
    Arc::new(
        SamkhyaTableProvider::new(inner)
            .with_column_stats(0, ColumnStats::new().with_row_count(rows)),
    )
}

fn fact(n: usize) -> Arc<MemTable> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("id", DataType::Int64, false),
        Field::new("a_id", DataType::Int32, false),
        Field::new("b_id", DataType::Int32, false),
        Field::new("c_id", DataType::Int32, false),
        Field::new("val", DataType::Int64, false),
    ]));
    let ids: Vec<i64> = (0..n as i64).collect();
    let a: Vec<i32> = (0..n as i32).map(|i| i % 5).collect();
    let b: Vec<i32> = (0..n as i32).map(|i| i % 7).collect();
    let c: Vec<i32> = (0..n as i32).map(|i| i % 3).collect();
    let v: Vec<i64> = (0..n as i64).map(|i| i * 2).collect();
    let batch = RecordBatch::try_new(
        Arc::clone(&schema),
        vec![
            Arc::new(Int64Array::from(ids)),
            Arc::new(Int32Array::from(a)),
            Arc::new(Int32Array::from(b)),
            Arc::new(Int32Array::from(c)),
            Arc::new(Int64Array::from(v)),
        ],
    )
    .unwrap();
    Arc::new(MemTable::try_new(schema, vec![vec![batch]]).unwrap())
}

fn dim(prefix: &str, n: i32, key: &str) -> Arc<MemTable> {
    let schema = Arc::new(Schema::new(vec![
        Field::new(key, DataType::Int32, false),
        Field::new("label", DataType::Utf8, false),
    ]));
    let keys: Vec<i32> = (0..n).collect();
    let labels: Vec<String> = (0..n).map(|i| format!("{prefix}_{i}")).collect();
    let label_refs: Vec<&str> = labels.iter().map(|s| s.as_str()).collect();
    let batch = RecordBatch::try_new(
        Arc::clone(&schema),
        vec![
            Arc::new(Int32Array::from(keys)),
            Arc::new(StringArray::from(label_refs)),
        ],
    )
    .unwrap();
    Arc::new(MemTable::try_new(schema, vec![vec![batch]]).unwrap())
}

fn nullable_keys() -> Arc<MemTable> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("k", DataType::Int32, true),
        Field::new("v", DataType::Int64, false),
    ]));
    let keys = Int32Array::from(vec![Some(1), None, Some(2), None, Some(3)]);
    let vals = Int64Array::from(vec![10_i64, 20, 30, 40, 50]);
    let batch =
        RecordBatch::try_new(Arc::clone(&schema), vec![Arc::new(keys), Arc::new(vals)]).unwrap();
    Arc::new(MemTable::try_new(schema, vec![vec![batch]]).unwrap())
}

fn empty_keys() -> Arc<MemTable> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("k", DataType::Int32, false),
        Field::new("v", DataType::Int64, false),
    ]));
    let batch = RecordBatch::try_new(
        Arc::clone(&schema),
        vec![
            Arc::new(Int32Array::from(Vec::<i32>::new())),
            Arc::new(Int64Array::from(Vec::<i64>::new())),
        ],
    )
    .unwrap();
    Arc::new(MemTable::try_new(schema, vec![vec![batch]]).unwrap())
}

async fn explain(ctx: &SessionContext, sql: &str) -> String {
    let df = ctx.sql(&format!("EXPLAIN {sql}")).await.unwrap();
    let batches = df.collect().await.unwrap();
    let mut out = String::new();
    for b in &batches {
        let col = b.column(1);
        let arr = col.as_any().downcast_ref::<StringArray>().unwrap();
        for i in 0..arr.len() {
            out.push_str(arr.value(i));
            out.push('\n');
        }
    }
    out
}

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    // (a) single-table filter
    {
        let ctx = samkhya_ctx();
        ctx.register_table(
            "fact",
            wrap(fact(1000) as Arc<dyn TableProvider>, 800) as Arc<dyn TableProvider>,
        )
        .unwrap();
        println!("=== (a) single-table filter ===");
        println!(
            "{}",
            explain(&ctx, "SELECT COUNT(*) FROM fact WHERE id < 100").await
        );
    }

    // (b) 2-way inner join
    {
        let ctx = samkhya_ctx();
        ctx.register_table(
            "fact",
            wrap(fact(500) as Arc<dyn TableProvider>, 480) as Arc<dyn TableProvider>,
        )
        .unwrap();
        ctx.register_table(
            "da",
            wrap(dim("a", 5, "a_id") as Arc<dyn TableProvider>, 5) as Arc<dyn TableProvider>,
        )
        .unwrap();
        println!("=== (b) 2-way inner join ===");
        println!(
            "{}",
            explain(
                &ctx,
                "SELECT f.id, da.label FROM fact f JOIN da ON f.a_id = da.a_id WHERE f.id < 50"
            )
            .await
        );
    }

    // (c) 3-way star join
    {
        let ctx = samkhya_ctx();
        ctx.register_table(
            "fact",
            wrap(fact(300) as Arc<dyn TableProvider>, 290) as Arc<dyn TableProvider>,
        )
        .unwrap();
        ctx.register_table(
            "da",
            wrap(dim("a", 5, "a_id") as Arc<dyn TableProvider>, 5) as Arc<dyn TableProvider>,
        )
        .unwrap();
        ctx.register_table(
            "db",
            wrap(dim("b", 7, "b_id") as Arc<dyn TableProvider>, 7) as Arc<dyn TableProvider>,
        )
        .unwrap();
        println!("=== (c) 3-way star join ===");
        println!(
            "{}",
            explain(
                &ctx,
                "SELECT f.id, da.label, db.label FROM fact f JOIN da ON f.a_id = da.a_id JOIN db ON f.b_id = db.b_id WHERE f.id < 20"
            )
            .await
        );
    }

    // (d) 4-way cycle join
    {
        let ctx = samkhya_ctx();
        ctx.register_table(
            "fact",
            wrap(fact(200) as Arc<dyn TableProvider>, 190) as Arc<dyn TableProvider>,
        )
        .unwrap();
        ctx.register_table(
            "da",
            wrap(dim("a", 5, "a_id") as Arc<dyn TableProvider>, 5) as Arc<dyn TableProvider>,
        )
        .unwrap();
        ctx.register_table(
            "db",
            wrap(dim("b", 7, "b_id") as Arc<dyn TableProvider>, 7) as Arc<dyn TableProvider>,
        )
        .unwrap();
        ctx.register_table(
            "dc",
            wrap(dim("c", 3, "c_id") as Arc<dyn TableProvider>, 3) as Arc<dyn TableProvider>,
        )
        .unwrap();
        println!("=== (d) 4-way cycle join ===");
        println!(
            "{}",
            explain(
                &ctx,
                "SELECT f.id, da.label, db.label, dc.label FROM fact f JOIN da ON f.a_id = da.a_id JOIN db ON f.b_id = db.b_id JOIN dc ON f.c_id = dc.c_id WHERE f.id < 15"
            )
            .await
        );
    }

    // (e) NULLs in join keys
    {
        let ctx = samkhya_ctx();
        ctx.register_table(
            "ln",
            wrap(nullable_keys() as Arc<dyn TableProvider>, 5) as Arc<dyn TableProvider>,
        )
        .unwrap();
        ctx.register_table(
            "rn",
            wrap(nullable_keys() as Arc<dyn TableProvider>, 5) as Arc<dyn TableProvider>,
        )
        .unwrap();
        println!("=== (e) join with NULL keys ===");
        println!(
            "{}",
            explain(&ctx, "SELECT COUNT(*) FROM ln JOIN rn ON ln.k = rn.k").await
        );
    }

    // (f) empty side
    {
        let ctx = samkhya_ctx();
        ctx.register_table(
            "lhs",
            wrap(fact(100) as Arc<dyn TableProvider>, 95) as Arc<dyn TableProvider>,
        )
        .unwrap();
        ctx.register_table(
            "ehs",
            wrap(empty_keys() as Arc<dyn TableProvider>, 0) as Arc<dyn TableProvider>,
        )
        .unwrap();
        println!("=== (f) join with empty side ===");
        println!(
            "{}",
            explain(
                &ctx,
                "SELECT COUNT(*) FROM lhs JOIN ehs ON lhs.a_id = ehs.k"
            )
            .await
        );
    }
}
