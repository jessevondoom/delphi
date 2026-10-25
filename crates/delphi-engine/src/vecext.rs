//! `sqlite-vec` registered for every SQLite connection this process opens (`specs/004` §3.3).
//!
//! The extension is compiled into the binary and linked statically, so there is nothing to load from disk and
//! nothing to download. It is registered once, before the first connection is opened.

use std::sync::Once;

static REGISTER: Once = Once::new();

/// Make `vec0` and the `vec_*` functions available to connections opened after this call.
pub fn register() {
    REGISTER.call_once(|| {
        // SAFETY: `sqlite3_vec_init` has the signature SQLite expects of an auto-extension entry point; the
        // transmute only erases the `fn()` placeholder type the crate declares it with.
        unsafe {
            rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute::<
                *const (),
                unsafe extern "C" fn(*mut rusqlite::ffi::sqlite3, *mut *mut std::os::raw::c_char, *const rusqlite::ffi::sqlite3_api_routines) -> std::os::raw::c_int,
            >(sqlite_vec::sqlite3_vec_init as *const ())));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::{params, Connection};

    fn blob(v: &[f32]) -> Vec<u8> {
        v.iter().flat_map(|x| x.to_le_bytes()).collect()
    }

    #[test]
    fn vec0_filters_by_metadata_before_it_ranks_and_orders_by_cosine_distance() {
        register();
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch("CREATE VIRTUAL TABLE v USING vec0(chunk_id INTEGER PRIMARY KEY, zone TEXT PARTITION KEY, status TEXT, embedding float[3] distance_metric=cosine);").unwrap();
        for (id, zone, status, e) in [(1, "specs", "active", [1.0, 0.0, 0.0]), (2, "specs", "active", [0.9, 0.1, 0.0]), (3, "memory", "active", [1.0, 0.0, 0.0]), (4, "specs", "superseded", [1.0, 0.0, 0.0])] {
            c.execute("INSERT INTO v(chunk_id, zone, status, embedding) VALUES (?1, ?2, ?3, ?4)", params![id, zone, status, blob(&e)]).unwrap();
        }
        let mut st = c.prepare("SELECT chunk_id, distance FROM v WHERE embedding MATCH ?1 AND k = 10 AND status = 'active' AND zone = 'specs' ORDER BY distance").unwrap();
        let rows: Vec<(i64, f64)> = st.query_map([blob(&[1.0, 0.0, 0.0])], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().flatten().collect();
        assert_eq!(rows.iter().map(|r| r.0).collect::<Vec<_>>(), vec![1, 2], "{:?}", rows);
        assert!(rows[0].1 < 1e-6 && rows[1].1 > rows[0].1);
    }
}
