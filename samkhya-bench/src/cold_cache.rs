//! Cold-cache eviction via `posix_fadvise(POSIX_FADV_DONTNEED)`.
//!
//! Cold-cache benchmark discipline (Leis et al., VLDB 2015 §3) requires
//! that the kernel page cache does not pre-warm subsequent trials with
//! data left over from earlier trials. The canonical recipe
//! (`sync && echo 3 > /proc/sys/vm/drop_caches`) requires root. This
//! module implements a root-free per-file eviction path using
//! `posix_fadvise(fd, 0, 0, POSIX_FADV_DONTNEED)`, which advises the
//! kernel that the file's contents will not be reused soon and lets a
//! plain user evict the pages backing that file from the page cache.
//!
//! Citation: Leis, V., Gubichev, A., Mirchev, A., Boncz, P., Kemper, A.,
//! and Neumann, T. "How Good Are Query Optimizers, Really?" PVLDB 9(3),
//! 2015, §3 — JOB experimental protocol, cold-cache amplification.
//!
//! ## Scope
//!
//! - Iterates `*.parquet` files in a directory (CSV path is no longer
//!   first-class for the JOB-Slow head-to-head; sidecars are byte-tiny
//!   and ride along with the parquet eviction in practice).
//! - Unix only. On non-unix builds the helper logs a one-line skip and
//!   returns 0 bytes evicted so the cold-cache CLI flag still type-checks.
//!
//! ## Why per-file, not per-page
//!
//! `POSIX_FADV_DONTNEED` with `len = 0` covers the entire file. We do
//! not slice by parquet row-group ranges because (a) the JOB workload
//! re-reads multiple unpredictable column slices per query and (b)
//! row-group-precise eviction would re-introduce a warm-cache bias on
//! columns the next query happens to revisit.

use std::path::Path;

use samkhya_core::Result;
use samkhya_core::error::Error;

/// Iterate every `*.parquet` file in `dir` and advise the kernel to drop
/// its page-cache pages via `posix_fadvise(POSIX_FADV_DONTNEED)`. Returns
/// the total number of bytes covered by the eviction call (file sizes
/// summed). The kernel's actual eviction is best-effort — `posix_fadvise`
/// returns success once the advice is queued, even if dirty pages or
/// pinned mmaps prevent immediate reclamation.
///
/// Skips non-parquet entries silently (CSV files in the same directory
/// would otherwise inflate the eviction set without a measurable effect
/// on the Parquet read path).
///
/// Logs one `[cold-cache] evicted <table>.parquet (Y bytes)` line per
/// successfully-advised file to stdout so the receipt has an audit
/// trail.
#[cfg(unix)]
pub fn evict_imdb_parquet_from_page_cache(dir: &Path) -> Result<u64> {
    use std::fs::File;
    use std::os::unix::io::AsRawFd;

    if !dir.is_dir() {
        return Err(Error::Feedback(format!(
            "cold_cache: not a directory: {}",
            dir.display()
        )));
    }

    let mut total_bytes: u64 = 0;
    let entries = std::fs::read_dir(dir)
        .map_err(|e| Error::Feedback(format!("cold_cache: read_dir({}): {e}", dir.display())))?;

    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        match path.extension().and_then(|e| e.to_str()) {
            Some("parquet") => {}
            _ => continue,
        }

        let file = match File::open(&path) {
            Ok(f) => f,
            Err(e) => {
                eprintln!(
                    "[cold-cache] WARN open({}) failed: {e}; skipping",
                    path.display()
                );
                continue;
            }
        };
        let len = file.metadata().map(|m| m.len()).unwrap_or(0);

        // posix_fadvise: signed offset/len in libc's signature.
        // Per POSIX: offset=0, len=0 → "from offset to end of file".
        // SAFETY: `file.as_raw_fd()` is a valid open fd held by `file`
        // for the duration of this call; the libc call itself takes no
        // pointers and only inspects the kernel inode for `fd`.
        let rc = unsafe { libc::posix_fadvise(file.as_raw_fd(), 0, 0, libc::POSIX_FADV_DONTNEED) };
        if rc != 0 {
            // posix_fadvise returns the errno directly (not via *errno*).
            eprintln!(
                "[cold-cache] WARN posix_fadvise({}) returned errno={rc}; skipping",
                path.display()
            );
            continue;
        }

        let table = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("<unknown>");
        println!("[cold-cache] evicted {} ({} bytes)", table, len);
        total_bytes = total_bytes.saturating_add(len);
    }

    Ok(total_bytes)
}

/// Non-unix fallback: cold-cache eviction via `posix_fadvise` is a
/// Unix-only API. On Windows/wasi we log a one-line skip and return 0
/// so the CLI flag still type-checks (warm-cache is the only achievable
/// mode on these platforms today; a Windows port would need
/// `FILE_FLAG_NO_BUFFERING` or `EmptyWorkingSet`).
#[cfg(not(unix))]
pub fn evict_imdb_parquet_from_page_cache(_dir: &Path) -> Result<u64> {
    eprintln!("[cold-cache] WARN posix_fadvise is unix-only; eviction skipped on this platform");
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Smoke test: create a 100 MiB temp parquet-shaped file (extension
    /// matters; content does not), call the eviction helper, assert the
    /// returned byte count matches.
    #[cfg(unix)]
    #[test]
    fn evict_returns_total_bytes_of_parquet_files() {
        use std::io::Write;

        let tmp = tempfile::tempdir().expect("tempdir");
        let dir = tmp.path();

        // 100 MiB exactly.
        const SIZE: u64 = 100 * 1024 * 1024;
        let path = dir.join("synthetic.parquet");
        let mut f = std::fs::File::create(&path).expect("create");
        // Write in 1 MiB chunks to keep peak RSS low.
        let chunk = vec![0u8; 1024 * 1024];
        for _ in 0..100 {
            f.write_all(&chunk).expect("write");
        }
        f.sync_all().expect("sync");
        drop(f);

        // Drop a sibling non-parquet file that must be skipped.
        std::fs::write(dir.join("ignore.csv"), b"x,y\n1,2\n").expect("csv");

        let evicted = evict_imdb_parquet_from_page_cache(dir).expect("evict");
        assert_eq!(
            evicted, SIZE,
            "evicted byte count must equal the parquet file size; CSV must be skipped"
        );
    }

    /// Non-existent directory → Err, not panic.
    #[cfg(unix)]
    #[test]
    fn evict_on_missing_dir_errors() {
        let res = evict_imdb_parquet_from_page_cache(Path::new("/nonexistent/dir/path"));
        assert!(res.is_err(), "missing dir must error");
    }
}
