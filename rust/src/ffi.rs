use std::ffi::{c_char, CStr};
use std::os::raw::c_void;

use crate::config::FfiSerialConfig;
use crate::enumerate::list_ports_json;
use crate::port::{free_buffer, CompletionCallback, Port, OP_OPEN};

#[no_mangle]
pub extern "C" fn asw_list_ports() -> *mut c_char {
    let json = list_ports_json();
    std::ffi::CString::new(json).unwrap().into_raw()
}

#[no_mangle]
pub extern "C" fn asw_free_string(ptr: *mut c_char) {
    if ptr.is_null() {
        return;
    }
    unsafe {
        drop(std::ffi::CString::from_raw(ptr));
    }
}

/// Opens a port. `path` is a NUL-terminated UTF-8 string (e.g. "COM3").
/// Calls `callback(req_id, OP_OPEN, error_code, data, data_len)` exactly once:
/// on success `data` points to 8 bytes containing the little-endian
/// `*mut Port` handle (data_len == 8); on failure data is null/0 and
/// error_code holds the Win32 error code.
#[no_mangle]
pub extern "C" fn asw_open(
    path: *const c_char,
    baud_rate: u32,
    data_bits: u8,
    stop_bits: u8,
    parity: u8,
    flow_control: u8,
    req_id: u64,
    callback: CompletionCallback,
) {
    let path = unsafe { CStr::from_ptr(path) }.to_string_lossy().to_string();
    std::thread::spawn(move || {
        let mut wide: Vec<u16> = format!("\\\\.\\{path}").encode_utf16().collect();
        wide.push(0);
        let cfg = FfiSerialConfig {
            baud_rate,
            data_bits,
            stop_bits,
            parity,
            flow_control,
        };
        match Port::open(&wide, cfg) {
            Ok(port_ptr) => {
                callback(req_id, OP_OPEN, 0, std::ptr::null_mut(), 0, port_ptr as u64);
            }
            Err(code) => {
                callback(req_id, OP_OPEN, code, std::ptr::null_mut(), 0, 0);
            }
        }
    });
}

/// Frees a buffer previously handed to a `CompletionCallback` (currently:
/// only `OP_READ` results). Must be called exactly once per buffer, after
/// the bytes have been copied out on the Dart side.
#[no_mangle]
pub extern "C" fn asw_free_buffer(ptr: *mut u8, len: u32) {
    unsafe { free_buffer(ptr, len) };
}

#[no_mangle]
pub extern "C" fn asw_read(
    port: *mut Port,
    max_bytes: u32,
    timeout_ms: i64,
    req_id: u64,
    callback: CompletionCallback,
) {
    let port = unsafe { &*port };
    port.read(req_id, max_bytes, timeout_ms, callback);
}

#[no_mangle]
pub extern "C" fn asw_write(
    port: *mut Port,
    data: *const u8,
    len: u32,
    req_id: u64,
    callback: CompletionCallback,
) {
    let port = unsafe { &*port };
    let slice = unsafe { std::slice::from_raw_parts(data, len as usize) };
    port.write(req_id, slice.to_vec(), callback);
}

#[no_mangle]
pub extern "C" fn asw_drain(port: *mut Port, req_id: u64, callback: CompletionCallback) {
    let port = unsafe { &*port };
    port.drain(req_id, callback);
}

#[no_mangle]
pub extern "C" fn asw_configure(
    port: *mut Port,
    baud_rate: u32,
    data_bits: u8,
    stop_bits: u8,
    parity: u8,
    flow_control: u8,
    req_id: u64,
    callback: CompletionCallback,
) {
    let port = unsafe { &*port };
    let cfg = FfiSerialConfig {
        baud_rate,
        data_bits,
        stop_bits,
        parity,
        flow_control,
    };
    port.configure(req_id, cfg, callback);
}

#[no_mangle]
pub extern "C" fn asw_close(port: *mut Port, req_id: u64, callback: CompletionCallback) {
    Port::close(port, req_id, callback);
}

// Referenced to keep the c_void import used when target-specific cfg trims other usages.
#[allow(dead_code)]
fn _keep(_: *mut c_void) {}
