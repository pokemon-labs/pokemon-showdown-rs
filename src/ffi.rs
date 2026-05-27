//! C Foreign Function Interface (FFI) for the Pokemon Showdown Battle Simulator
//!
//! This module exposes a stable C API for embedding the battle simulator in
//! non-Rust programs (Python, C, C++, Go, etc.).
//!
//! ## Design Philosophy
//!
//! - All functions are `extern "C"` with `#[no_mangle]`
//! - Strings cross the boundary as null-terminated C strings (`*const c_char` / `*mut c_char`)
//! - All heap-allocated objects are returned as opaque pointers and must be freed
//!   via the corresponding `_free` function
//! - Functions that can fail return `PsResult` (0 = success, non-zero = error)
//! - The last error string is retrievable with `ps_get_last_error()`
//! - Thread-local error state avoids global mutexes
//!
//! ## Typical Usage Flow
//!
//! ```c
//! // 1. Create a stream
//! PsBattleStream *stream = ps_stream_new();
//!
//! // 2. Send initialization protocol
//! ps_stream_receive(stream, ">start {\"formatid\":\"gen9randombattle\",\"seed\":\"gen5,1234abcd\"}");
//! ps_stream_receive(stream, ">player p1 {\"name\":\"Alice\",\"team\":\"\"}");
//! ps_stream_receive(stream, ">player p2 {\"name\":\"Bob\",\"team\":\"\"}");
//!
//! // 3. Poll output
//! char *output;
//! while ((output = ps_stream_read(stream)) != NULL) {
//!     printf("%s\n", output);
//!     ps_string_free(output);
//! }
//!
//! // 4. Send choices
//! ps_stream_receive(stream, ">p1 move 1");
//! ps_stream_receive(stream, ">p2 move 2");
//!
//! // 5. Clean up
//! ps_stream_free(stream);
//! ```

#![allow(clippy::not_unsafe_ptr_arg_deref)]

use std::cell::RefCell;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};

use crate::battle_stream::{BattleStream, BattleStreamOptions, ReplayMode};

// ---------------------------------------------------------------------------
// Error handling — thread-local last-error string
// ---------------------------------------------------------------------------

thread_local! {
    static LAST_ERROR: RefCell<Option<CString>> = const { RefCell::new(None) };
}

/// Store an error message in thread-local storage and return the given code.
fn set_error(msg: impl Into<Vec<u8>>, code: c_int) -> c_int {
    LAST_ERROR.with(|e| {
        *e.borrow_mut() = CString::new(msg).ok();
    });
    code
}

fn clear_error() {
    LAST_ERROR.with(|e| *e.borrow_mut() = None);
}

// ---------------------------------------------------------------------------
// Error codes
// ---------------------------------------------------------------------------

/// Success
pub const PS_OK: c_int = 0;
/// Null pointer passed where a valid pointer was required
pub const PS_ERR_NULL_PTR: c_int = -1;
/// Invalid UTF-8 or malformed C string
pub const PS_ERR_INVALID_STRING: c_int = -2;
/// Internal Rust panic or other unexpected error
pub const PS_ERR_INTERNAL: c_int = -3;

// ---------------------------------------------------------------------------
// Opaque handle types
// ---------------------------------------------------------------------------

/// Opaque handle to a `BattleStream`. Obtain with [`ps_stream_new`] or
/// [`ps_stream_new_with_options`] and release with [`ps_stream_free`].
#[repr(C)]
pub struct PsBattleStream(BattleStream);

// ---------------------------------------------------------------------------
// String helpers
// ---------------------------------------------------------------------------

/// Safely convert a raw `*const c_char` to a `&str`, setting the thread-local
/// error and returning `None` on failure.
unsafe fn cstr_to_str<'a>(ptr: *const c_char) -> Option<&'a str> {
    if ptr.is_null() {
        set_error("null pointer passed for string argument", PS_ERR_NULL_PTR);
        return None;
    }
    match CStr::from_ptr(ptr).to_str() {
        Ok(s) => Some(s),
        Err(e) => {
            set_error(format!("invalid UTF-8 string: {e}"), PS_ERR_INVALID_STRING);
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Error API
// ---------------------------------------------------------------------------

/// Retrieve the last error message produced by any `ps_*` call on this thread.
///
/// Returns a pointer to a null-terminated UTF-8 string, or NULL if no error
/// has occurred. **The returned pointer is valid until the next `ps_*` call on
/// the same thread** — copy it before making further calls.
///
/// # Safety
/// The returned pointer must not be freed by the caller.
#[no_mangle]
pub unsafe extern "C" fn ps_get_last_error() -> *const c_char {
    LAST_ERROR.with(|e| match &*e.borrow() {
        Some(s) => s.as_ptr(),
        None => std::ptr::null(),
    })
}

// ---------------------------------------------------------------------------
// BattleStream lifecycle
// ---------------------------------------------------------------------------

/// Create a new `BattleStream` with default options.
///
/// Returns a pointer to the opaque stream handle, or NULL on allocation
/// failure. The caller owns the returned pointer and must eventually call
/// [`ps_stream_free`].
#[no_mangle]
pub extern "C" fn ps_stream_new() -> *mut PsBattleStream {
    clear_error();
    let stream = BattleStream::new();
    Box::into_raw(Box::new(PsBattleStream(stream)))
}

/// Create a new `BattleStream` with explicit options.
///
/// # Parameters
/// - `debug`: non-zero to enable verbose debug output
/// - `no_catch`: non-zero to disable internal error catching (panics propagate)
/// - `replay_mode`: `0` = off, `1` = on, `2` = full
/// - `keep_alive`: non-zero to keep the stream alive after the battle ends
///
/// Returns NULL on allocation failure.
#[no_mangle]
pub extern "C" fn ps_stream_new_with_options(
    debug: c_int,
    no_catch: c_int,
    replay_mode: c_int,
    keep_alive: c_int,
) -> *mut PsBattleStream {
    clear_error();
    let replay = match replay_mode {
        1 => ReplayMode::Spectator,
        2 => ReplayMode::Full,
        _ => ReplayMode::Off,
    };
    let stream = BattleStream::with_options(BattleStreamOptions {
        debug: debug != 0,
        no_catch: no_catch != 0,
        replay,
        keep_alive: keep_alive != 0,
    });
    Box::into_raw(Box::new(PsBattleStream(stream)))
}

/// Free a `BattleStream` created by [`ps_stream_new`] or
/// [`ps_stream_new_with_options`].
///
/// Passing NULL is a no-op.
///
/// # Safety
/// `stream` must be a pointer previously returned by `ps_stream_new*` and must
/// not have been freed already.
#[no_mangle]
pub unsafe extern "C" fn ps_stream_free(stream: *mut PsBattleStream) {
    if !stream.is_null() {
        drop(Box::from_raw(stream));
    }
}

// ---------------------------------------------------------------------------
// BattleStream I/O
// ---------------------------------------------------------------------------

/// Send a chunk of protocol data to the battle stream.
///
/// `chunk` must be a null-terminated UTF-8 string containing one or more
/// newline-separated protocol lines, e.g.:
///
/// ```text
/// >start {"formatid":"gen9randombattle"}
/// ```
///
/// Returns `PS_OK` (0) on success or a negative error code on failure.
///
/// # Safety
/// `stream` and `chunk` must be valid non-null pointers.
#[no_mangle]
pub unsafe extern "C" fn ps_stream_receive(
    stream: *mut PsBattleStream,
    chunk: *const c_char,
) -> c_int {
    clear_error();
    if stream.is_null() {
        return set_error("ps_stream_receive: stream is null", PS_ERR_NULL_PTR);
    }
    let s = match cstr_to_str(chunk) {
        Some(s) => s,
        None => return PS_ERR_INVALID_STRING,
    };
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        (*stream).0.receive(s);
    }));
    match result {
        Ok(()) => PS_OK,
        Err(e) => {
            let msg = if let Some(s) = e.downcast_ref::<&str>() {
                format!("panic in ps_stream_receive: {s}")
            } else if let Some(s) = e.downcast_ref::<String>() {
                format!("panic in ps_stream_receive: {s}")
            } else {
                "panic in ps_stream_receive: unknown".to_string()
            };
            set_error(msg, PS_ERR_INTERNAL)
        }
    }
}

/// Read one output message from the battle stream.
///
/// Returns a heap-allocated, null-terminated UTF-8 string that the caller
/// **must** free with [`ps_string_free`], or NULL when there are no more
/// messages available right now.
///
/// Call in a loop until NULL is returned to drain all pending output after
/// each call to [`ps_stream_receive`].
///
/// # Safety
/// `stream` must be a valid non-null pointer.
#[no_mangle]
pub unsafe extern "C" fn ps_stream_read(stream: *mut PsBattleStream) -> *mut c_char {
    clear_error();
    if stream.is_null() {
        set_error("ps_stream_read: stream is null", PS_ERR_NULL_PTR);
        return std::ptr::null_mut();
    }
    match (*stream).0.read() {
        Some(msg) => match CString::new(msg) {
            Ok(cs) => cs.into_raw(),
            Err(_) => {
                set_error("ps_stream_read: output contained null byte", PS_ERR_INTERNAL);
                std::ptr::null_mut()
            }
        },
        None => std::ptr::null_mut(),
    }
}

/// Convenience wrapper: send a player choice to the stream.
///
/// Equivalent to calling [`ps_stream_receive`] with `>p1 <choice>` or
/// `>p2 <choice>` depending on `player_slot` (1 or 2).
///
/// Returns `PS_OK` on success.
///
/// # Safety
/// `stream` and `choice` must be valid non-null pointers.
#[no_mangle]
pub unsafe extern "C" fn ps_stream_choose(
    stream: *mut PsBattleStream,
    player_slot: c_int,
    choice: *const c_char,
) -> c_int {
    clear_error();
    if stream.is_null() {
        return set_error("ps_stream_choose: stream is null", PS_ERR_NULL_PTR);
    }
    let choice_str = match cstr_to_str(choice) {
        Some(s) => s,
        None => return PS_ERR_INVALID_STRING,
    };
    let line = format!(">p{player_slot} {choice_str}");
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        (*stream).0.receive(&line);
    }));
    match result {
        Ok(()) => PS_OK,
        Err(_) => set_error("panic in ps_stream_choose", PS_ERR_INTERNAL),
    }
}

/// Returns non-zero if the battle has ended.
///
/// # Safety
/// `stream` must be a valid non-null pointer.
#[no_mangle]
pub unsafe extern "C" fn ps_stream_is_ended(stream: *const PsBattleStream) -> c_int {
    clear_error();
    if stream.is_null() {
        set_error("ps_stream_is_ended: stream is null", PS_ERR_NULL_PTR);
        return 0;
    }
    if (*stream).0.ended() { 1 } else { 0 }
}

/// Returns the winner's name as a heap-allocated string, or NULL if the battle
/// has not ended yet or ended in a tie.
///
/// The caller must free the returned string with [`ps_string_free`].
///
/// # Safety
/// `stream` must be a valid non-null pointer.
#[no_mangle]
pub unsafe extern "C" fn ps_stream_winner(stream: *const PsBattleStream) -> *mut c_char {
    clear_error();
    if stream.is_null() {
        set_error("ps_stream_winner: stream is null", PS_ERR_NULL_PTR);
        return std::ptr::null_mut();
    }
    match (*stream).0.winner() {
        Some(name) => match CString::new(name) {
            Ok(cs) => cs.into_raw(),
            Err(_) => std::ptr::null_mut(),
        },
        None => std::ptr::null_mut(),
    }
}

// ---------------------------------------------------------------------------
// String memory management
// ---------------------------------------------------------------------------

/// Free a string returned by any `ps_*` function.
///
/// Passing NULL is a no-op. Do **not** use this on strings you allocated
/// yourself — only on pointers returned by this library.
///
/// # Safety
/// `s` must be a pointer previously returned by a `ps_*` function that
/// allocates a string (e.g. `ps_stream_read`, `ps_stream_winner`).
#[no_mangle]
pub unsafe extern "C" fn ps_string_free(s: *mut c_char) {
    if !s.is_null() {
        drop(CString::from_raw(s));
    }
}

// ---------------------------------------------------------------------------
// Version / diagnostics
// ---------------------------------------------------------------------------

/// Returns a static null-terminated string with the simulator version, e.g.
/// `"pokemon-showdown-rs 0.1.0"`. The returned pointer is valid for the
/// lifetime of the process and must **not** be freed.
#[no_mangle]
pub extern "C" fn ps_version() -> *const c_char {
    // SAFETY: static string, valid for 'static
    static VERSION: &[u8] = b"pokemon-showdown-rs 0.1.0\0";
    VERSION.as_ptr() as *const c_char
}
