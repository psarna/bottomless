#![allow(non_snake_case)]
#![allow(clippy::not_unsafe_ptr_arg_deref)]

mod ffi;
mod replicator;

use crate::ffi::{libsql_wal_methods, sqlite3_file, sqlite3_vfs, PgHdr, Wal};
use std::ffi::c_void;

pub extern "C" fn xOpen(
    vfs: *const sqlite3_vfs,
    db_file: *mut sqlite3_file,
    wal_name: *const i8,
    no_shm_mode: i32,
    max_size: i64,
    methods: *mut libsql_wal_methods,
    wal: *mut *const Wal,
) -> i32 {
    tracing::trace!("Opening {}", unsafe {
        std::ffi::CStr::from_ptr(wal_name).to_str().unwrap()
    });
    let orig_methods = unsafe { &*(*methods).underlying_methods };
    let new_methods = unsafe { &mut *methods };

    let db_path = match unsafe { std::ffi::CStr::from_ptr(wal_name).to_str() } {
        Ok(s) => {
            if s.ends_with("-wal") {
                &s[0..s.len() - 4]
            } else {
                s
            }
        }
        Err(e) => {
            tracing::error!("Failed to parse file name: {}", e);
            return ffi::SQLITE_CANTOPEN;
        }
    };
    new_methods.replicator.register_db(db_path);

    let mut native_db_size: i64 = 0;
    unsafe {
        ((*(*db_file).methods).xFileSize)(db_file, &mut native_db_size as *mut i64);
    }
    tracing::warn!(
        "Native file size: {} ({} pages)",
        native_db_size,
        native_db_size / replicator::Replicator::PAGE_SIZE as i64
    );

    let rc = (orig_methods.xOpen)(vfs, db_file, wal_name, no_shm_mode, max_size, methods, wal);
    if rc != ffi::SQLITE_OK {
        return rc;
    }

    let mut native_wal_size: i64 = 0;
    unsafe {
        let wal_file = (*(*wal)).wal_fd;
        ((*(*wal_file).methods).xFileSize)(wal_file, &mut native_wal_size as *mut i64);
    }
    tracing::warn!(
        "Native -wal file size: {} ({} pages)",
        native_wal_size,
        native_wal_size / replicator::Replicator::PAGE_SIZE as i64
    );

    if native_db_size == 0 && native_wal_size == 0 {
        tracing::info!("Restoring data from bottomless storage");
        tracing::error!("Not implemented yet");
    }

    tracing::warn!(
        "Generation {} ({:?})",
        new_methods.replicator.generation,
        new_methods.replicator.generation.get_timestamp()
    );

    /* TODO:
        1. -wal file present -> refuse to start, checkpoint first
        2a. Main database file not empty:
            a. Create a generation timeuuid
            b. Upload the database file to timeuuid/file
            c. Start a new backup session
        2b. Main database file empty:
            a. Get latest timeuuid
            b. Restore the main file + WAL logs, up to the marker
    */

    ffi::SQLITE_OK
}

fn get_orig_methods(wal: *mut Wal) -> &'static libsql_wal_methods {
    unsafe { &*((*(*wal).wal_methods).underlying_methods) }
}

fn get_methods(wal: *mut Wal) -> &'static mut libsql_wal_methods {
    unsafe { &mut *((*wal).wal_methods) }
}

pub extern "C" fn xClose(
    wal: *mut Wal,
    db: *mut c_void,
    sync_flags: i32,
    n_buf: i32,
    z_buf: *mut u8,
) -> i32 {
    tracing::debug!("Closing wal");
    let orig_methods = get_orig_methods(wal);
    (orig_methods.xClose)(wal, db, sync_flags, n_buf, z_buf)
}

pub extern "C" fn xLimit(wal: *mut Wal, limit: i64) {
    let orig_methods = get_orig_methods(wal);
    (orig_methods.xLimit)(wal, limit)
}

pub extern "C" fn xBeginReadTransaction(wal: *mut Wal, changed: *mut i32) -> i32 {
    let orig_methods = get_orig_methods(wal);
    (orig_methods.xBeginReadTransaction)(wal, changed)
}

pub extern "C" fn xEndReadTransaction(wal: *mut Wal) -> i32 {
    let orig_methods = get_orig_methods(wal);
    (orig_methods.xEndReadTransaction)(wal)
}

pub extern "C" fn xFindFrame(wal: *mut Wal, pgno: i32, frame: *mut i32) -> i32 {
    let orig_methods = get_orig_methods(wal);
    (orig_methods.xFindFrame)(wal, pgno, frame)
}

pub extern "C" fn xReadFrame(wal: *mut Wal, frame: u32, n_out: i32, p_out: *mut u8) -> i32 {
    let orig_methods = get_orig_methods(wal);
    (orig_methods.xReadFrame)(wal, frame, n_out, p_out)
}

pub extern "C" fn xDbSize(wal: *mut Wal) -> i32 {
    let orig_methods = get_orig_methods(wal);
    (orig_methods.xDbSize)(wal)
}

pub extern "C" fn xBeginWriteTransaction(wal: *mut Wal) -> i32 {
    let orig_methods = get_orig_methods(wal);
    (orig_methods.xBeginWriteTransaction)(wal)
}

pub extern "C" fn xEndWriteTransaction(wal: *mut Wal) -> i32 {
    let orig_methods = get_orig_methods(wal);
    (orig_methods.xEndWriteTransaction)(wal)
}

pub extern "C" fn xUndo(
    wal: *mut Wal,
    func: extern "C" fn(*mut c_void, i32) -> i32,
    ctx: *mut c_void,
) -> i32 {
    let orig_methods = get_orig_methods(wal);
    (orig_methods.xUndo)(wal, func, ctx)
}

pub extern "C" fn xSavepoint(wal: *mut Wal, wal_data: *mut u32) {
    let orig_methods = get_orig_methods(wal);
    (orig_methods.xSavepoint)(wal, wal_data)
}

pub extern "C" fn xSavepointUndo(wal: *mut Wal, wal_data: *mut u32) -> i32 {
    let orig_methods = get_orig_methods(wal);
    (orig_methods.xSavepointUndo)(wal, wal_data)
}

pub extern "C" fn xFrames(
    wal: *mut Wal,
    page_size: u32,
    page_headers: *const PgHdr,
    size_after: i32,
    is_commit: i32,
    sync_flags: i32,
) -> i32 {
    let methods = get_methods(wal);
    let orig_methods = get_orig_methods(wal);
    for (pgno, data) in ffi::PageHdrIter::new(page_headers, page_size as usize) {
        methods.replicator.write(pgno, data);
    }
    if is_commit != 0 {
        match methods.replicator.commit() {
            Ok(()) => (),
            Err(e) => {
                tracing::error!("Failed to replicate: {}", e);
                return ffi::SQLITE_IOERR_WRITE;
            }
        }
    }
    (orig_methods.xFrames)(
        wal,
        page_size,
        page_headers,
        size_after,
        is_commit,
        sync_flags,
    )
}

pub extern "C" fn xCheckpoint(
    wal: *mut Wal,
    db: *mut c_void,
    emode: i32,
    busy_handler: extern "C" fn(busy_param: *mut c_void) -> i32,
    sync_flags: i32,
    n_buf: i32,
    z_buf: *mut u8,
    frames_in_wal: *mut i32,
    backfilled_frames: *mut i32,
) -> i32 {
    let orig_methods = get_orig_methods(wal);
    (orig_methods.xCheckpoint)(
        wal,
        db,
        emode,
        busy_handler,
        sync_flags,
        n_buf,
        z_buf,
        frames_in_wal,
        backfilled_frames,
    )
}

pub extern "C" fn xCallback(wal: *mut Wal) -> i32 {
    let orig_methods = get_orig_methods(wal);
    (orig_methods.xCallback)(wal)
}

pub extern "C" fn xExclusiveMode(wal: *mut Wal) -> i32 {
    let orig_methods = get_orig_methods(wal);
    (orig_methods.xExclusiveMode)(wal)
}

pub extern "C" fn xHeapMemory(wal: *mut Wal) -> i32 {
    let orig_methods = get_orig_methods(wal);
    (orig_methods.xHeapMemory)(wal)
}

pub extern "C" fn xFile(wal: *mut Wal) -> *const c_void {
    let orig_methods = get_orig_methods(wal);
    (orig_methods.xFile)(wal)
}

pub extern "C" fn xDb(wal: *mut Wal, db: *const c_void) {
    let orig_methods = get_orig_methods(wal);
    (orig_methods.xDb)(wal, db)
}

pub extern "C" fn xPathnameLen(orig_len: i32) -> i32 {
    orig_len + 4
}

pub extern "C" fn xGetPathname(buf: *mut u8, orig: *const u8, orig_len: i32) {
    unsafe { std::ptr::copy(orig, buf, orig_len as usize) }
    unsafe { std::ptr::copy("-wal".as_ptr(), buf.offset(orig_len as isize), 4) }
}

#[no_mangle]
pub extern "C" fn bottomless_init() {
    tracing_subscriber::fmt::init();
    tracing::debug!("bottomless module initialized");
}

#[tracing::instrument]
#[no_mangle]
pub extern "C" fn bottomless_methods(
    underlying_methods: *const libsql_wal_methods,
) -> *const libsql_wal_methods {
    let vwal_name: *const u8 = "bottomless\0".as_ptr();
    let replicator = match replicator::Replicator::new() {
        Ok(repl) => repl,
        Err(e) => {
            tracing::error!("Failed to initialize replicator: {}", e);
            return std::ptr::null();
        }
    };

    Box::into_raw(Box::new(libsql_wal_methods {
        xOpen,
        xClose,
        xLimit,
        xBeginReadTransaction,
        xEndReadTransaction,
        xFindFrame,
        xReadFrame,
        xDbSize,
        xBeginWriteTransaction,
        xEndWriteTransaction,
        xUndo,
        xSavepoint,
        xSavepointUndo,
        xFrames,
        xCheckpoint,
        xCallback,
        xExclusiveMode,
        xHeapMemory,
        xFile,
        xDb,
        xPathnameLen,
        xGetPathname,
        name: vwal_name,
        b_uses_shm: 0,
        p_next: std::ptr::null(),
        underlying_methods,
        replicator,
    }))
}
