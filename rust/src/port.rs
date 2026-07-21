use std::ptr::null_mut;
use std::sync::mpsc::{channel, Sender};
use std::thread::JoinHandle;

use windows_sys::Win32::Devices::Communication::*;
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Storage::FileSystem::*;
use windows_sys::Win32::System::Threading::*;
use windows_sys::Win32::System::IO::*;

use crate::config::{apply_to_dcb, FfiSerialConfig};

/// `data`/`data_len` describe a heap buffer allocated with [`alloc_buffer`];
/// the Dart side must call `asw_free_buffer` on it after copying, exactly
/// once, once it has read the bytes. This indirection exists because
/// `NativeCallable.listener` posts to the Dart event loop asynchronously and
/// returns immediately, so anything stack-local in this crate would already
/// be gone by the time Dart inspects the pointer — everything crossing the
/// boundary must be heap-allocated and independently freed.
/// `value` carries an inline 64-bit payload (currently: the `*mut Port`
/// handle on `OP_OPEN`) that doesn't need the buffer indirection.
pub type CompletionCallback =
    extern "C" fn(req_id: u64, op: i32, error_code: i32, data: *mut u8, data_len: u32, value: u64);

/// Leaks `bytes` as a thin, Dart-freeable pointer. Pair with `free_buffer`.
pub fn alloc_buffer(bytes: &[u8]) -> (*mut u8, u32) {
    let mut v = bytes.to_vec();
    v.shrink_to_fit();
    let ptr = v.as_mut_ptr();
    let len = v.len() as u32;
    std::mem::forget(v);
    (ptr, len)
}

/// # Safety
/// `ptr`/`len` must be exactly the pair returned by a prior `alloc_buffer`
/// call, and must be freed at most once.
pub unsafe fn free_buffer(ptr: *mut u8, len: u32) {
    if ptr.is_null() {
        return;
    }
    drop(Vec::from_raw_parts(ptr, len as usize, len as usize));
}

#[cfg(test)]
mod buffer_tests {
    use super::*;

    #[test]
    fn round_trips_bytes_through_alloc_and_free() {
        let original = b"hello overlapped world".to_vec();
        let (ptr, len) = alloc_buffer(&original);
        assert_eq!(len as usize, original.len());
        let copied = unsafe { std::slice::from_raw_parts(ptr, len as usize) }.to_vec();
        assert_eq!(copied, original);
        unsafe { free_buffer(ptr, len) };
    }

    #[test]
    fn empty_slice_round_trips_to_a_zero_length_non_dangling_alloc() {
        let (ptr, len) = alloc_buffer(&[]);
        assert_eq!(len, 0);
        unsafe { free_buffer(ptr, len) };
    }

    #[test]
    fn free_buffer_on_null_is_a_no_op() {
        unsafe { free_buffer(null_mut(), 0) };
    }
}

pub const OP_OPEN: i32 = 0;
pub const OP_READ: i32 = 1;
pub const OP_WRITE: i32 = 2;
pub const OP_DRAIN: i32 = 3;
pub const OP_CONFIGURE: i32 = 4;
pub const OP_CLOSE: i32 = 5;

/// Wraps a raw HANDLE so it can be moved into worker threads. Safe because
/// Win32 file/serial handles may be used concurrently from multiple threads
/// as long as each in-flight operation has its own OVERLAPPED structure,
/// which is exactly how the reader/writer threads use it here.
#[derive(Clone, Copy)]
struct RawHandle(HANDLE);
unsafe impl Send for RawHandle {}
unsafe impl Sync for RawHandle {}

enum ReaderCmd {
    Read {
        req_id: u64,
        max_bytes: u32,
        timeout_ms: i64,
        callback: CompletionCallback,
    },
    Stop,
}

enum WriterCmd {
    Write {
        req_id: u64,
        data: Vec<u8>,
        callback: CompletionCallback,
    },
    Drain {
        req_id: u64,
        callback: CompletionCallback,
    },
    Configure {
        req_id: u64,
        cfg: FfiSerialConfig,
        callback: CompletionCallback,
    },
    /// Sentinel telling `writer_loop` to stop. Carries no callback: the
    /// close completion is only reported once the closer thread (see
    /// `Port::close`) has actually joined both worker threads and called
    /// `CloseHandle` — firing it here, as soon as the writer merely
    /// acknowledges the message, would let Dart start a new `open()` on the
    /// same COM port while the handle is still alive underneath it.
    Close,
}

pub struct Port {
    handle: RawHandle,
    stop_event: RawHandle,
    reader_tx: Sender<ReaderCmd>,
    writer_tx: Sender<WriterCmd>,
    reader_thread: Option<JoinHandle<()>>,
    writer_thread: Option<JoinHandle<()>>,
}

fn make_overlapped(event: HANDLE) -> OVERLAPPED {
    let mut ov: OVERLAPPED = unsafe { std::mem::zeroed() };
    ov.hEvent = event;
    ov
}

fn last_error() -> i32 {
    unsafe { GetLastError() as i32 }
}

impl Port {
    pub fn open(path_wide: &[u16], cfg: FfiSerialConfig) -> Result<*mut Port, i32> {
        unsafe {
            let handle = CreateFileW(
                path_wide.as_ptr(),
                (GENERIC_READ | GENERIC_WRITE) as u32,
                0,
                null_mut(),
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED,
                std::ptr::null_mut(),
            );
            if handle == INVALID_HANDLE_VALUE {
                return Err(last_error());
            }

            let mut dcb: DCB = std::mem::zeroed();
            dcb.DCBlength = std::mem::size_of::<DCB>() as u32;
            if GetCommState(handle, &mut dcb) == 0 {
                let err = last_error();
                CloseHandle(handle);
                return Err(err);
            }
            apply_to_dcb(&mut dcb, &cfg);
            if SetCommState(handle, &dcb) == 0 {
                let err = last_error();
                CloseHandle(handle);
                return Err(err);
            }

            let mut timeouts: COMMTIMEOUTS = std::mem::zeroed();
            // Placeholder; the reader thread sets the real per-call
            // COMMTIMEOUTS mode (see set_timeouts_* in this module) before
            // every ReadFile, since the right mode depends on that read's
            // requested timeout.
            timeouts.ReadIntervalTimeout = 0;
            timeouts.ReadTotalTimeoutMultiplier = 0;
            timeouts.ReadTotalTimeoutConstant = 0;
            timeouts.WriteTotalTimeoutMultiplier = 0;
            timeouts.WriteTotalTimeoutConstant = 0;
            if SetCommTimeouts(handle, &timeouts) == 0 {
                let err = last_error();
                CloseHandle(handle);
                return Err(err);
            }

            let stop_event = CreateEventW(null_mut(), 1, 0, null_mut());
            let handle = RawHandle(handle);
            let stop = RawHandle(stop_event);

            let (reader_tx, reader_rx) = channel::<ReaderCmd>();
            let (writer_tx, writer_rx) = channel::<WriterCmd>();

            let reader_handle = handle;
            let reader_stop = stop;
            let reader_thread = std::thread::spawn(move || reader_loop(reader_handle, reader_stop, reader_rx));

            let writer_handle = handle;
            let writer_stop = stop;
            let writer_thread = std::thread::spawn(move || writer_loop(writer_handle, writer_stop, writer_rx));

            let port = Box::new(Port {
                handle,
                stop_event: stop,
                reader_tx,
                writer_tx,
                reader_thread: Some(reader_thread),
                writer_thread: Some(writer_thread),
            });

            Ok(Box::into_raw(port))
        }
    }

    pub fn read(&self, req_id: u64, max_bytes: u32, timeout_ms: i64, callback: CompletionCallback) {
        let _ = self.reader_tx.send(ReaderCmd::Read {
            req_id,
            max_bytes,
            timeout_ms,
            callback,
        });
    }

    pub fn write(&self, req_id: u64, data: Vec<u8>, callback: CompletionCallback) {
        let _ = self.writer_tx.send(WriterCmd::Write { req_id, data, callback });
    }

    pub fn drain(&self, req_id: u64, callback: CompletionCallback) {
        let _ = self.writer_tx.send(WriterCmd::Drain { req_id, callback });
    }

    pub fn configure(&self, req_id: u64, cfg: FfiSerialConfig, callback: CompletionCallback) {
        let _ = self.writer_tx.send(WriterCmd::Configure { req_id, cfg, callback });
    }

    /// Spawns a dedicated closer thread so the FFI call itself never blocks;
    /// the calling (Dart) thread gets control back immediately and the
    /// completion callback fires once teardown has actually finished.
    pub fn close(port_ptr: *mut Port, req_id: u64, callback: CompletionCallback) {
        // `*mut Port` isn't Send; smuggle it across the thread boundary as a
        // plain address and reconstitute it inside the closure. Safe because
        // the original raw pointer (from Box::into_raw in `open`) is only
        // ever touched from this one closer thread from here on.
        let addr = port_ptr as usize;
        std::thread::spawn(move || {
            let mut port = unsafe { Box::from_raw(addr as *mut Port) };
            let _ = port.writer_tx.send(WriterCmd::Close);
            // Wakes the reader thread if it's idle at `rx.recv()` (no read
            // in flight); if a read *is* in flight, the SetEvent/CancelIoEx
            // below cancels it instead — either way the thread returns.
            let _ = port.reader_tx.send(ReaderCmd::Stop);

            unsafe {
                SetEvent(port.stop_event.0);
                CancelIoEx(port.handle.0, null_mut());
            }

            if let Some(t) = port.reader_thread.take() {
                let _ = t.join();
            }
            if let Some(t) = port.writer_thread.take() {
                let _ = t.join();
            }
            unsafe {
                CloseHandle(port.handle.0);
                CloseHandle(port.stop_event.0);
            }
            // `port` (the Box) drops here, freeing the allocation. Only now,
            // with the OS handle genuinely closed, is it safe to tell Dart
            // the close finished (e.g. so it can reopen the same COM port).
            callback(req_id, OP_CLOSE, 0, null_mut(), 0, 0);
        });
    }
}

enum ReadOutcome {
    Cancelled,
    Done(u32),
    Failed(i32),
}

/// Issues one `ReadFile` and waits for it to complete (or be cancelled via
/// `stop_event`). Whether/how long this actually blocks is entirely
/// controlled by the handle's current `COMMTIMEOUTS` (see the `set_*_mode`
/// helpers below) — `wait_ms` just bounds the local `WaitForMultipleObjects`
/// and should normally be `INFINITE` since the driver-level timeout already
/// does the real bounding.
fn do_single_read(handle: RawHandle, stop_event: RawHandle, buf: &mut [u8], wait_ms: u32) -> ReadOutcome {
    let read_event = unsafe { CreateEventW(null_mut(), 1, 0, null_mut()) };
    let mut ov = make_overlapped(read_event);
    let mut bytes_read: u32 = 0;

    let ok = unsafe { ReadFile(handle.0, buf.as_mut_ptr(), buf.len() as u32, &mut bytes_read, &mut ov) };

    let outcome = if ok != 0 {
        ReadOutcome::Done(bytes_read)
    } else {
        let err = unsafe { GetLastError() };
        if err == ERROR_IO_PENDING {
            let wait_handles = [stop_event.0, read_event];
            let wait = unsafe { WaitForMultipleObjects(wait_handles.len() as u32, wait_handles.as_ptr(), 0, wait_ms) };
            if wait == WAIT_OBJECT_0 {
                unsafe { CancelIoEx(handle.0, &mut ov) };
                ReadOutcome::Cancelled
            } else if wait == WAIT_OBJECT_0 + 1 {
                let mut transferred: u32 = 0;
                let ok2 = unsafe { GetOverlappedResult(handle.0, &ov, &mut transferred, 0) };
                if ok2 != 0 {
                    ReadOutcome::Done(transferred)
                } else {
                    ReadOutcome::Failed(unsafe { GetLastError() } as i32)
                }
            } else {
                // Local wait_ms elapsed (only relevant if a caller passes a
                // finite value); cancel and report whatever made it through.
                unsafe { CancelIoEx(handle.0, &mut ov) };
                let mut transferred: u32 = 0;
                unsafe { GetOverlappedResult(handle.0, &ov, &mut transferred, 1) };
                ReadOutcome::Done(transferred)
            }
        } else {
            ReadOutcome::Failed(err as i32)
        }
    };

    unsafe { CloseHandle(read_event) };
    outcome
}

/// `ReadFile` blocks until the requested byte count is fully read (no
/// timeout). Used for the first byte of an untimed `read()`.
fn set_timeouts_block_for_count(handle: RawHandle) {
    let mut t: COMMTIMEOUTS = unsafe { std::mem::zeroed() };
    t.ReadIntervalTimeout = 0;
    t.ReadTotalTimeoutMultiplier = 0;
    t.ReadTotalTimeoutConstant = 0;
    unsafe { SetCommTimeouts(handle.0, &t) };
}

/// `ReadFile` returns immediately with whatever is already buffered (possibly
/// zero bytes). Used to opportunistically drain extra buffered bytes after
/// the first one arrives, and for a caller-requested zero timeout (poll).
fn set_timeouts_poll(handle: RawHandle) {
    let mut t: COMMTIMEOUTS = unsafe { std::mem::zeroed() };
    t.ReadIntervalTimeout = u32::MAX;
    t.ReadTotalTimeoutMultiplier = 0;
    t.ReadTotalTimeoutConstant = 0;
    unsafe { SetCommTimeouts(handle.0, &t) };
}

/// The documented `COMMTIMEOUTS` special case for "return as soon as at
/// least one byte is available, or time out after `constant_ms`". This is
/// exactly the `read()` contract, so the bounded-timeout path is a single
/// `ReadFile` call.
fn set_timeouts_bounded(handle: RawHandle, constant_ms: u32) {
    let mut t: COMMTIMEOUTS = unsafe { std::mem::zeroed() };
    t.ReadIntervalTimeout = u32::MAX;
    t.ReadTotalTimeoutMultiplier = u32::MAX;
    t.ReadTotalTimeoutConstant = constant_ms.clamp(1, u32::MAX - 1);
    unsafe { SetCommTimeouts(handle.0, &t) };
}

fn reader_loop(handle: RawHandle, stop_event: RawHandle, rx: std::sync::mpsc::Receiver<ReaderCmd>) {
    loop {
        let cmd = match rx.recv() {
            Ok(c) => c,
            Err(_) => return,
        };
        let (req_id, max_bytes, timeout_ms, callback) = match cmd {
            ReaderCmd::Stop => return,
            ReaderCmd::Read { req_id, max_bytes, timeout_ms, callback } => (req_id, max_bytes, timeout_ms, callback),
        };

        let mut buf = vec![0u8; max_bytes.max(1) as usize];

        let result: Option<Result<u32, i32>> = if timeout_ms < 0 {
            // No timeout: block indefinitely for the first byte, then grab
            // whatever else is already sitting in the input buffer.
            set_timeouts_block_for_count(handle);
            match do_single_read(handle, stop_event, &mut buf[..1], INFINITE) {
                ReadOutcome::Cancelled => None,
                ReadOutcome::Failed(e) => Some(Err(e)),
                ReadOutcome::Done(_) => {
                    if buf.len() > 1 {
                        set_timeouts_poll(handle);
                        match do_single_read(handle, stop_event, &mut buf[1..], INFINITE) {
                            ReadOutcome::Done(more) => Some(Ok(1 + more)),
                            ReadOutcome::Cancelled | ReadOutcome::Failed(_) => Some(Ok(1)),
                        }
                    } else {
                        Some(Ok(1))
                    }
                }
            }
        } else if timeout_ms == 0 {
            set_timeouts_poll(handle);
            match do_single_read(handle, stop_event, &mut buf, INFINITE) {
                ReadOutcome::Done(n) => Some(Ok(n)),
                ReadOutcome::Cancelled => None,
                ReadOutcome::Failed(e) => Some(Err(e)),
            }
        } else {
            set_timeouts_bounded(handle, timeout_ms as u32);
            match do_single_read(handle, stop_event, &mut buf, INFINITE) {
                ReadOutcome::Done(n) => Some(Ok(n)),
                ReadOutcome::Cancelled => None,
                ReadOutcome::Failed(e) => Some(Err(e)),
            }
        };

        match result {
            None => return, // shutting down
            Some(Ok(n)) => {
                let (ptr, len) = alloc_buffer(&buf[..n as usize]);
                callback(req_id, OP_READ, 0, ptr, len, 0);
            }
            Some(Err(code)) => callback(req_id, OP_READ, code, null_mut(), 0, 0),
        }
    }
}

fn write_all(handle: RawHandle, stop_event: RawHandle, data: &[u8]) -> Result<(), i32> {
    let event = unsafe { CreateEventW(null_mut(), 1, 0, null_mut()) };
    let mut offset = 0usize;
    let result = (|| {
        while offset < data.len() {
            let mut ov = make_overlapped(event);
            let mut written: u32 = 0;
            let chunk = &data[offset..];
            let ok = unsafe {
                WriteFile(
                    handle.0,
                    chunk.as_ptr(),
                    chunk.len() as u32,
                    &mut written,
                    &mut ov,
                )
            };
            if ok == 0 {
                let err = unsafe { GetLastError() };
                if err == ERROR_IO_PENDING {
                    let wait_handles = [stop_event.0, event];
                    let wait = unsafe {
                        WaitForMultipleObjects(wait_handles.len() as u32, wait_handles.as_ptr(), 0, INFINITE)
                    };
                    if wait == WAIT_OBJECT_0 {
                        unsafe { CancelIoEx(handle.0, &mut ov) };
                        return Err(ERROR_OPERATION_ABORTED as i32);
                    }
                    let mut transferred: u32 = 0;
                    let ok2 = unsafe { GetOverlappedResult(handle.0, &ov, &mut transferred, 0) };
                    if ok2 == 0 {
                        return Err(unsafe { GetLastError() } as i32);
                    }
                    written = transferred;
                } else {
                    return Err(err as i32);
                }
            }
            offset += written as usize;
        }
        Ok(())
    })();
    unsafe { CloseHandle(event) };
    result
}

fn writer_loop(handle: RawHandle, stop_event: RawHandle, rx: std::sync::mpsc::Receiver<WriterCmd>) {
    loop {
        let cmd = match rx.recv() {
            Ok(c) => c,
            Err(_) => return,
        };
        match cmd {
            WriterCmd::Write { req_id, data, callback } => match write_all(handle, stop_event, &data) {
                Ok(()) => callback(req_id, OP_WRITE, 0, null_mut(), 0, 0),
                Err(code) => callback(req_id, OP_WRITE, code, null_mut(), 0, 0),
            },
            WriterCmd::Drain { req_id, callback } => {
                let ok = unsafe { FlushFileBuffers(handle.0) };
                if ok != 0 {
                    callback(req_id, OP_DRAIN, 0, null_mut(), 0, 0);
                } else {
                    callback(req_id, OP_DRAIN, unsafe { GetLastError() } as i32, null_mut(), 0, 0);
                }
            }
            WriterCmd::Configure { req_id, cfg, callback } => unsafe {
                let mut dcb: DCB = std::mem::zeroed();
                dcb.DCBlength = std::mem::size_of::<DCB>() as u32;
                if GetCommState(handle.0, &mut dcb) == 0 {
                    callback(req_id, OP_CONFIGURE, GetLastError() as i32, null_mut(), 0, 0);
                    continue;
                }
                apply_to_dcb(&mut dcb, &cfg);
                if SetCommState(handle.0, &dcb) == 0 {
                    callback(req_id, OP_CONFIGURE, GetLastError() as i32, null_mut(), 0, 0);
                } else {
                    callback(req_id, OP_CONFIGURE, 0, null_mut(), 0, 0);
                }
            },
            WriterCmd::Close => return,
        }
    }
}
