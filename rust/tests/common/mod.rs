//! Shared harness for hardware-in-the-loop integration tests.
//!
//! `Port`'s read/write/drain/configure/close all report completion via a
//! plain `extern "C" fn` callback (no closures — that's the FFI boundary
//! contract Dart also uses), so this harness reproduces the same
//! req_id -> pending-completion bookkeeping the Dart side does, just backed
//! by an `mpsc::channel` instead of a `Completer`.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use async_serial_win::config::FfiSerialConfig;
use async_serial_win::port::{free_buffer, CompletionCallback, Port};

pub struct CallbackResult {
    pub error_code: i32,
    pub data: Vec<u8>,
    /// Only meaningful for `OP_OPEN` (the opened `*mut Port`, as an address);
    /// unused by the wrappers below since `common::open` calls `Port::open`
    /// directly instead of going through the callback path.
    #[allow(dead_code)]
    pub value: u64,
}

fn registry() -> &'static Mutex<HashMap<u64, Sender<CallbackResult>>> {
    static REGISTRY: OnceLock<Mutex<HashMap<u64, Sender<CallbackResult>>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

fn next_req_id() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

extern "C" fn test_callback(req_id: u64, _op: i32, error_code: i32, data: *mut u8, data_len: u32, value: u64) {
    let bytes = if data_len > 0 {
        let v = unsafe { std::slice::from_raw_parts(data, data_len as usize) }.to_vec();
        unsafe { free_buffer(data, data_len) };
        v
    } else {
        Vec::new()
    };
    let tx = registry().lock().unwrap().remove(&req_id);
    if let Some(tx) = tx {
        let _ = tx.send(CallbackResult { error_code, data: bytes, value });
    }
}

/// Default timeout for the *test harness itself* waiting on a callback —
/// this exists purely so a real library bug (a completion that never fires)
/// fails the test instead of hanging `cargo test` forever. It's intentionally
/// generous and unrelated to any read/drain/etc timeout under test.
const HARNESS_TIMEOUT: Duration = Duration::from_secs(10);

fn submit_and_wait<F: FnOnce(u64, CompletionCallback)>(f: F) -> CallbackResult {
    try_submit_and_wait(f, HARNESS_TIMEOUT)
        .unwrap_or_else(|| panic!("operation did not complete within {HARNESS_TIMEOUT:?}"))
}

/// Like `submit_and_wait`, but returns `None` on timeout instead of
/// panicking. Used by `discover_pair`'s probing, where trying a port that
/// turns out to be uncooperative real hardware (not a loopback pair) should
/// just be skipped, not fail the whole test run. Note this still leaks the
/// pending native-side operation/thread if it never completes — acceptable
/// for a short-lived test-probing process, not something the public helpers
/// below do (those are expected to always complete or indicate a real bug).
fn try_submit_and_wait<F: FnOnce(u64, CompletionCallback)>(f: F, timeout: Duration) -> Option<CallbackResult> {
    let req_id = next_req_id();
    let (tx, rx) = mpsc::channel();
    registry().lock().unwrap().insert(req_id, tx);
    f(req_id, test_callback);
    rx.recv_timeout(timeout).ok()
}

pub fn open(path: &str, cfg: FfiSerialConfig) -> Result<*mut Port, i32> {
    let mut wide: Vec<u16> = format!("\\\\.\\{path}").encode_utf16().collect();
    wide.push(0);
    Port::open(&wide, cfg)
}

pub fn read(port: &Port, max_bytes: u32, timeout_ms: i64) -> CallbackResult {
    submit_and_wait(|req_id, cb| port.read(req_id, max_bytes, timeout_ms, cb))
}

pub fn write(port: &Port, data: &[u8]) -> CallbackResult {
    submit_and_wait(|req_id, cb| port.write(req_id, data.to_vec(), cb))
}

pub fn drain(port: &Port) -> CallbackResult {
    submit_and_wait(|req_id, cb| port.drain(req_id, cb))
}

pub fn configure(port: &Port, cfg: FfiSerialConfig) -> CallbackResult {
    submit_and_wait(|req_id, cb| port.configure(req_id, cfg, cb))
}

pub fn close(port_ptr: *mut Port) -> CallbackResult {
    submit_and_wait(|req_id, cb| Port::close(port_ptr, req_id, cb))
}

/// Probing-only variants that return `None` on timeout instead of panicking
/// (see `try_submit_and_wait`). Used exclusively by `discover_pair`.
fn try_write(port: &Port, data: &[u8], timeout: Duration) -> Option<CallbackResult> {
    try_submit_and_wait(|req_id, cb| port.write(req_id, data.to_vec(), cb), timeout)
}

fn try_read(port: &Port, max_bytes: u32, timeout_ms: i64, harness_timeout: Duration) -> Option<CallbackResult> {
    try_submit_and_wait(|req_id, cb| port.read(req_id, max_bytes, timeout_ms, cb), harness_timeout)
}

fn try_close(port_ptr: *mut Port, timeout: Duration) -> Option<CallbackResult> {
    try_submit_and_wait(|req_id, cb| Port::close(port_ptr, req_id, cb), timeout)
}

pub fn default_config() -> FfiSerialConfig {
    FfiSerialConfig {
        baud_rate: 115_200,
        data_bits: 8,
        stop_bits: 0,
        parity: 0,
        flow_control: 0,
    }
}

/// Reads until `expected_len` bytes have accumulated or `overall_timeout`
/// elapses. Mirrors the Dart-side `_readExactly` test helper: a single
/// `read()` call is allowed to return as soon as the first byte shows up,
/// without waiting for the rest of a chunked/relayed write, so exact-length
/// assertions need to accumulate across possibly-multiple reads.
pub fn read_exactly(port: &Port, expected_len: usize, overall_timeout: Duration) -> Vec<u8> {
    let deadline = std::time::Instant::now() + overall_timeout;
    let mut bytes = Vec::new();
    while bytes.len() < expected_len {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        let result = read(port, (expected_len - bytes.len()) as u32, remaining.as_millis() as i64);
        if result.error_code != 0 || result.data.is_empty() {
            break;
        }
        bytes.extend_from_slice(&result.data);
    }
    bytes
}

/// Probing-only counterpart to `read_exactly`: bails out (returning whatever
/// was accumulated so far) instead of panicking if a `read()` call itself
/// never completes, since a non-cooperative real hardware port hanging is an
/// expected outcome while probing, not a test failure.
fn try_read_exactly(port: &Port, expected_len: usize, overall_timeout: Duration) -> Vec<u8> {
    let deadline = std::time::Instant::now() + overall_timeout;
    let mut bytes = Vec::new();
    while bytes.len() < expected_len {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        let harness_wait = remaining + Duration::from_secs(1);
        let Some(result) = try_read(port, (expected_len - bytes.len()) as u32, remaining.as_millis() as i64, harness_wait) else {
            break;
        };
        if result.error_code != 0 || result.data.is_empty() {
            break;
        }
        bytes.extend_from_slice(&result.data);
    }
    bytes
}

/// Finds a loopback (e.g. com0com) pair by brute-force probing every
/// unordered pair of enumerated ports: open both, try writing a marker in
/// each direction, and see if it round-trips. Panics with a clear message if
/// none is found, since these tests assume a null-modem pair is present.
pub fn discover_pair() -> (String, String) {
    let ports_json = async_serial_win::enumerate::list_ports_json();
    let entries: Vec<serde_json::Value> = serde_json::from_str(&ports_json).expect("list_ports_json produced invalid JSON");
    let paths: Vec<String> = entries
        .into_iter()
        .filter_map(|entry| entry.get("path").and_then(|p| p.as_str()).map(|s| s.to_string()))
        .collect();

    const MARKER: &[u8] = b"async_serial_win-rust-probe";

    for i in 0..paths.len() {
        for j in (i + 1)..paths.len() {
            let (path_a, path_b) = (&paths[i], &paths[j]);
            let cfg = default_config();

            let port_a = match open(path_a, cfg) {
                Ok(p) => p,
                Err(_) => continue,
            };
            let port_b = match open(path_b, cfg) {
                Ok(p) => p,
                Err(_) => {
                    try_close(port_a, Duration::from_secs(3));
                    continue;
                }
            };

            let mut found = None;
            for (writer, reader, writer_path, reader_path) in [
                (port_a, port_b, path_a.clone(), path_b.clone()),
                (port_b, port_a, path_b.clone(), path_a.clone()),
            ] {
                let writer_ref = unsafe { &*writer };
                let reader_ref = unsafe { &*reader };
                let Some(write_result) = try_write(writer_ref, MARKER, Duration::from_secs(2)) else {
                    continue;
                };
                if write_result.error_code != 0 {
                    continue;
                }
                let received = try_read_exactly(reader_ref, MARKER.len(), Duration::from_secs(2));
                if received == MARKER {
                    found = Some((writer_path, reader_path));
                    break;
                }
            }

            // Best-effort cleanup while probing: if a non-cooperative real
            // port also hangs on close, don't let that abort the whole
            // discovery loop (a genuine close() hang against the actual
            // loopback pair would still be caught by the real test
            // scenarios, which use the strict, panicking `close`).
            try_close(port_a, Duration::from_secs(3));
            try_close(port_b, Duration::from_secs(3));

            if let Some(pair) = found {
                return pair;
            }
        }
    }

    panic!(
        "No loopback pair found among {:?}. These tests require a virtual \
         null-modem pair (e.g. com0com) to be installed.",
        paths
    );
}
