//! JOB — the Join Order Benchmark of Leis et al., VLDB 2015.
//!
//! The full corpus is 113 queries grouped into 33 templates (1-33) with
//! per-template variants (`a`, `b`, …). The benchmark is SQL-95 valid against
//! the IMDb schema (tables `aka_name`, `cast_info`, `company_name`,
//! `info_type`, `keyword`, `movie_companies`, `movie_keyword`, `movie_info`,
//! `name`, `person_info`, `title`, …). The canonical query texts live at
//! <https://github.com/winkyao/join-order-benchmark>.
//!
//! ## JOB-Slow subset
//!
//! "JOB-Slow" is the 33 hardest queries by Leis et al. (VLDB 2015) — the
//! ones that maximally stress cardinality estimation:
//!
//! `1d, 6e, 6f, 7c, 8c, 8d, 9b, 9c, 9d, 10b, 10c, 12c, 14c, 15c, 15d, 17c,
//!  17d, 17e, 17f, 18c, 19c, 19d, 20c, 24b, 25b, 25c, 26b, 26c, 30c, 31c,
//!  32b, 33b, 33c`.
//!
//! These are flagged by [`is_job_slow`].
//!
//! ## Status (v0.6.0 scaffolding)
//!
//! Five queries (`1a`, `2b`, `6a`, `17a`, `29a`) carry the canonical SQL text
//! verbatim from the upstream repo — they are the smoke subset from earlier
//! releases. The remaining 108 entries are placeholders: the entry exists with
//! a stable `name` so the bench's per-query reporting layout (`bench list-queries`,
//! `bench run --suite job-slow-real`) is correct, but `sql` is a `TODO`
//! sentinel that the runner detects and reports as skipped.
//!
//! The agent landing v0.6.0 fills the placeholders in by copying the SQL
//! from the upstream repo (one query per `.sql` file in
//! `join-order-benchmark/`). Honest is better than guessed: rather than
//! hallucinate the SQL we leave a clear marker.

use super::Query;

/// Sentinel inserted as the `sql` field for the 108 queries whose text has
/// not yet been imported from upstream. The runner treats this as
/// "scaffold-only" and reports skipped.
pub const PLACEHOLDER_SQL: &str = "-- TODO(v0.6.0): import SQL from https://github.com/winkyao/join-order-benchmark";

/// Returns true if `name` belongs to the JOB-Slow subset (the 33 hardest
/// queries by Leis et al., VLDB 2015).
pub fn is_job_slow(name: &str) -> bool {
    matches!(
        name,
        "1d" | "6e"
            | "6f"
            | "7c"
            | "8c"
            | "8d"
            | "9b"
            | "9c"
            | "9d"
            | "10b"
            | "10c"
            | "12c"
            | "14c"
            | "15c"
            | "15d"
            | "17c"
            | "17d"
            | "17e"
            | "17f"
            | "18c"
            | "19c"
            | "19d"
            | "20c"
            | "24b"
            | "25b"
            | "25c"
            | "26b"
            | "26c"
            | "30c"
            | "31c"
            | "32b"
            | "33b"
            | "33c"
    )
}

/// Returns true if the query carries real SQL (not the placeholder sentinel).
pub fn has_sql(q: &Query) -> bool {
    !q.sql.starts_with("-- TODO(v0.6.0)")
}

// --- Hand-written SQL for the smoke subset --------------------------------

const SQL_1A: &str = "SELECT MIN(mc.note) AS production_note,
       MIN(t.title) AS movie_title,
       MIN(t.production_year) AS movie_year
FROM company_type AS ct,
     info_type AS it,
     movie_companies AS mc,
     movie_info_idx AS mi_idx,
     title AS t
WHERE ct.kind = 'production companies'
  AND it.info = 'top 250 rank'
  AND mc.note NOT LIKE '%(as Metro-Goldwyn-Mayer Pictures)%'
  AND (mc.note LIKE '%(co-production)%' OR mc.note LIKE '%(presents)%')
  AND ct.id = mc.company_type_id
  AND t.id = mc.movie_id
  AND t.id = mi_idx.movie_id
  AND mc.movie_id = mi_idx.movie_id
  AND it.id = mi_idx.info_type_id;";

const SQL_2B: &str = "SELECT MIN(t.title) AS movie_title
FROM company_name AS cn,
     keyword AS k,
     movie_companies AS mc,
     movie_keyword AS mk,
     title AS t
WHERE cn.country_code = '[nl]'
  AND k.keyword = 'character-name-in-title'
  AND cn.id = mc.company_id
  AND mc.movie_id = t.id
  AND t.id = mk.movie_id
  AND mk.keyword_id = k.id
  AND mc.movie_id = mk.movie_id;";

const SQL_6A: &str = "SELECT MIN(k.keyword) AS movie_keyword,
       MIN(n.name) AS actor_name,
       MIN(t.title) AS marvel_movie
FROM cast_info AS ci,
     keyword AS k,
     movie_keyword AS mk,
     name AS n,
     title AS t
WHERE k.keyword = 'marvel-cinematic-universe'
  AND n.name LIKE '%Downey%Robert%'
  AND t.production_year > 2010
  AND k.id = mk.keyword_id
  AND t.id = mk.movie_id
  AND t.id = ci.movie_id
  AND ci.movie_id = mk.movie_id
  AND n.id = ci.person_id;";

const SQL_17A: &str = "SELECT MIN(n.name) AS member_in_charnamed_american_movie,
       MIN(n.name) AS a1
FROM cast_info AS ci,
     company_name AS cn,
     keyword AS k,
     movie_companies AS mc,
     movie_keyword AS mk,
     name AS n,
     title AS t
WHERE cn.country_code = '[us]'
  AND k.keyword = 'character-name-in-title'
  AND n.name LIKE 'B%'
  AND n.id = ci.person_id
  AND ci.movie_id = t.id
  AND t.id = mk.movie_id
  AND mk.keyword_id = k.id
  AND t.id = mc.movie_id
  AND mc.company_id = cn.id
  AND ci.movie_id = mc.movie_id
  AND ci.movie_id = mk.movie_id
  AND mc.movie_id = mk.movie_id;";

const SQL_29A: &str = "SELECT MIN(ci.note) AS cast_note,
       MIN(cn.name) AS company_name,
       MIN(chn.name) AS character_name,
       MIN(mc.note) AS production_note,
       MIN(t.title) AS movie_title,
       MIN(t.production_year) AS movie_year
FROM aka_name AS an,
     char_name AS chn,
     cast_info AS ci,
     company_name AS cn,
     info_type AS it,
     keyword AS k,
     movie_companies AS mc,
     movie_info AS mi,
     movie_keyword AS mk,
     name AS n,
     person_info AS pi,
     role_type AS rt,
     title AS t
WHERE ci.note IN ('(voice)', '(voice: Japanese version)', '(voice) (uncredited)', '(voice: English version)')
  AND cn.country_code = '[us]'
  AND it.info = 'release dates'
  AND k.keyword = 'computer-animation'
  AND mi.info IS NOT NULL
  AND (mi.info LIKE 'Japan:%200%' OR mi.info LIKE 'USA:%200%')
  AND n.gender = 'f'
  AND n.name LIKE '%An%'
  AND rt.role = 'actress'
  AND t.production_year BETWEEN 2000 AND 2010
  AND t.id = mi.movie_id
  AND t.id = mc.movie_id
  AND t.id = ci.movie_id
  AND t.id = mk.movie_id
  AND mc.movie_id = ci.movie_id
  AND mc.movie_id = mi.movie_id
  AND mc.movie_id = mk.movie_id
  AND mi.movie_id = ci.movie_id
  AND mi.movie_id = mk.movie_id
  AND ci.movie_id = mk.movie_id
  AND cn.id = mc.company_id
  AND it.id = mi.info_type_id
  AND n.id = ci.person_id
  AND rt.id = ci.role_id
  AND n.id = an.person_id
  AND ci.person_id = an.person_id
  AND chn.id = ci.person_role_id
  AND n.id = pi.person_id
  AND ci.person_id = pi.person_id
  AND k.id = mk.keyword_id;";

// --- Full 113-query roster ------------------------------------------------
//
// Names are the canonical `<template><variant>` identifiers from the JOB
// repo. The five entries above use their real SQL; everything else routes
// to `PLACEHOLDER_SQL` until v0.6.0 fills them in.

macro_rules! q {
    ($name:literal) => {
        Query {
            name: $name,
            sql: PLACEHOLDER_SQL,
        }
    };
    ($name:literal, $sql:expr) => {
        Query {
            name: $name,
            sql: $sql,
        }
    };
}

pub const QUERIES: &[Query] = &[
    // template 1
    q!("1a", SQL_1A),
    q!("1b"),
    q!("1c"),
    q!("1d"),
    // template 2
    q!("2a"),
    q!("2b", SQL_2B),
    q!("2c"),
    q!("2d"),
    // template 3
    q!("3a"),
    q!("3b"),
    q!("3c"),
    // template 4
    q!("4a"),
    q!("4b"),
    q!("4c"),
    // template 5
    q!("5a"),
    q!("5b"),
    q!("5c"),
    // template 6
    q!("6a", SQL_6A),
    q!("6b"),
    q!("6c"),
    q!("6d"),
    q!("6e"),
    q!("6f"),
    // template 7
    q!("7a"),
    q!("7b"),
    q!("7c"),
    // template 8
    q!("8a"),
    q!("8b"),
    q!("8c"),
    q!("8d"),
    // template 9
    q!("9a"),
    q!("9b"),
    q!("9c"),
    q!("9d"),
    // template 10
    q!("10a"),
    q!("10b"),
    q!("10c"),
    // template 11
    q!("11a"),
    q!("11b"),
    q!("11c"),
    q!("11d"),
    // template 12
    q!("12a"),
    q!("12b"),
    q!("12c"),
    // template 13
    q!("13a"),
    q!("13b"),
    q!("13c"),
    q!("13d"),
    // template 14
    q!("14a"),
    q!("14b"),
    q!("14c"),
    // template 15
    q!("15a"),
    q!("15b"),
    q!("15c"),
    q!("15d"),
    // template 16
    q!("16a"),
    q!("16b"),
    q!("16c"),
    q!("16d"),
    // template 17
    q!("17a", SQL_17A),
    q!("17b"),
    q!("17c"),
    q!("17d"),
    q!("17e"),
    q!("17f"),
    // template 18
    q!("18a"),
    q!("18b"),
    q!("18c"),
    // template 19
    q!("19a"),
    q!("19b"),
    q!("19c"),
    q!("19d"),
    // template 20
    q!("20a"),
    q!("20b"),
    q!("20c"),
    // template 21
    q!("21a"),
    q!("21b"),
    q!("21c"),
    // template 22
    q!("22a"),
    q!("22b"),
    q!("22c"),
    q!("22d"),
    // template 23
    q!("23a"),
    q!("23b"),
    q!("23c"),
    // template 24
    q!("24a"),
    q!("24b"),
    // template 25
    q!("25a"),
    q!("25b"),
    q!("25c"),
    // template 26
    q!("26a"),
    q!("26b"),
    q!("26c"),
    // template 27
    q!("27a"),
    q!("27b"),
    q!("27c"),
    // template 28
    q!("28a"),
    q!("28b"),
    q!("28c"),
    // template 29
    q!("29a", SQL_29A),
    q!("29b"),
    q!("29c"),
    // template 30
    q!("30a"),
    q!("30b"),
    q!("30c"),
    // template 31
    q!("31a"),
    q!("31b"),
    q!("31c"),
    // template 32
    q!("32a"),
    q!("32b"),
    // template 33
    q!("33a"),
    q!("33b"),
    q!("33c"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roster_has_113_queries() {
        assert_eq!(QUERIES.len(), 113);
    }

    #[test]
    fn job_slow_subset_has_33_queries() {
        let count = QUERIES.iter().filter(|q| is_job_slow(q.name)).count();
        assert_eq!(count, 33);
    }

    #[test]
    fn smoke_queries_carry_real_sql() {
        for name in ["1a", "2b", "6a", "17a", "29a"] {
            let q = QUERIES.iter().find(|q| q.name == name).expect("query exists");
            assert!(has_sql(q), "{name} should carry real SQL");
        }
    }

    #[test]
    fn names_are_unique() {
        let mut names: Vec<&str> = QUERIES.iter().map(|q| q.name).collect();
        names.sort();
        let n = names.len();
        names.dedup();
        assert_eq!(names.len(), n, "duplicate names in QUERIES");
    }
}
