//! Hardware-in-the-loop integration tests. These require a virtual
//! null-modem pair (e.g. com0com) installed on the machine running `cargo
//! test` — `common::discover_pair()` finds it by brute-force probing rather
//! than hardcoding port names, so this works across machines.
//!
//! Everything lives in a single `#[test]` function. `cargo test` runs test
//! functions concurrently by default, and every scenario here needs
//! exclusive use of the same two physical/virtual COM ports, so splitting
//! into multiple `#[test]` fns would just race them against each other for
//! the same hardware. Sub-scenarios are still isolated: each opens and
//! closes its own port handles.

mod common;

use std::time::{Duration, Instant};

use async_serial_win::config::FfiSerialConfig;
use common::{close, configure, default_config, discover_pair, drain, open, read, read_exactly, write};

#[test]
fn loopback_pair_scenarios() {
    let (path_a, path_b) = discover_pair();
    eprintln!("Using loopback pair: {path_a} <-> {path_b}");

    macro_rules! run {
        ($name:ident($($arg:expr),*)) => {{
            eprintln!("-> {}", stringify!($name));
            $name($($arg),*);
            eprintln!("   ok");
        }};
    }

    run!(basic_round_trip(&path_a, &path_b));
    run!(concurrent_pending_read_and_write(&path_a, &path_b));
    run!(read_timeout_with_no_data_returns_empty(&path_a));
    run!(read_zero_timeout_polls_without_blocking(&path_a));
    run!(read_with_no_timeout_blocks_until_data_arrives(&path_a, &path_b));
    run!(drain_completes_without_error(&path_a, &path_b));
    run!(configure_changes_baud_rate(&path_a, &path_b));
    run!(close_releases_the_port_for_immediate_reopen(&path_a));
    run!(opposite_direction_also_round_trips(&path_a, &path_b));
    run!(repeated_write_read_cycles_do_not_leak_or_hang(&path_a, &path_b));
    run!(larger_payload_is_read_back_in_full(&path_a, &path_b));
    run!(opening_a_nonexistent_port_reports_an_error());
}

fn basic_round_trip(path_a: &str, path_b: &str) {
    let a = open(path_a, default_config()).expect("open a");
    let b = open(path_b, default_config()).expect("open b");
    let a_ref = unsafe { &*a };
    let b_ref = unsafe { &*b };

    let payload = b"hello overlapped world";
    let write_result = write(a_ref, payload);
    assert_eq!(write_result.error_code, 0, "write should succeed");

    let received = read_exactly(b_ref, payload.len(), Duration::from_secs(2));
    assert_eq!(received, payload);

    assert_eq!(close(a).error_code, 0);
    assert_eq!(close(b).error_code, 0);
}

fn concurrent_pending_read_and_write(path_a: &str, path_b: &str) {
    // read() must be able to be pending (blocked, waiting for data) at the
    // same time a write() is in flight on the other port — this is the
    // core "not polling" guarantee of the library.
    let a = open(path_a, default_config()).expect("open a");
    let b = open(path_b, default_config()).expect("open b");
    let a_ref = unsafe { &*a };
    let b_ref = unsafe { &*b };

    let payload = b"concurrent-probe";
    std::thread::scope(|scope| {
        let reader = scope.spawn(|| read(a_ref, payload.len() as u32, 3000));
        // Give the reader a moment to actually reach the pending ReadFile
        // before we write, so this exercises the blocking-until-data path
        // rather than a write-then-read race.
        std::thread::sleep(Duration::from_millis(150));
        let write_result = write(b_ref, payload);
        assert_eq!(write_result.error_code, 0);
        let read_result = reader.join().unwrap();
        assert_eq!(read_result.error_code, 0);
        assert_eq!(read_result.data, payload);
    });

    assert_eq!(close(a).error_code, 0);
    assert_eq!(close(b).error_code, 0);
}

fn read_timeout_with_no_data_returns_empty(path_a: &str) {
    let a = open(path_a, default_config()).expect("open a");
    let a_ref = unsafe { &*a };

    let start = Instant::now();
    let result = read(a_ref, 64, 500);
    let elapsed = start.elapsed();

    assert_eq!(result.error_code, 0);
    assert!(result.data.is_empty(), "expected an empty buffer on timeout, got {:?}", result.data);
    assert!(elapsed >= Duration::from_millis(400), "should have actually waited, elapsed={elapsed:?}");
    assert!(elapsed < Duration::from_secs(3), "should not wait much longer than the requested timeout, elapsed={elapsed:?}");

    assert_eq!(close(a).error_code, 0);
}

fn read_zero_timeout_polls_without_blocking(path_a: &str) {
    let a = open(path_a, default_config()).expect("open a");
    let a_ref = unsafe { &*a };

    let start = Instant::now();
    let result = read(a_ref, 64, 0);
    let elapsed = start.elapsed();

    assert_eq!(result.error_code, 0);
    assert!(result.data.is_empty());
    assert!(elapsed < Duration::from_millis(300), "a zero-timeout read should return almost immediately, elapsed={elapsed:?}");

    assert_eq!(close(a).error_code, 0);
}

fn read_with_no_timeout_blocks_until_data_arrives(path_a: &str, path_b: &str) {
    let a = open(path_a, default_config()).expect("open a");
    let b = open(path_b, default_config()).expect("open b");
    let a_ref = unsafe { &*a };
    let b_ref = unsafe { &*b };

    let payload = b"no-timeout-probe";
    std::thread::scope(|scope| {
        // timeout_ms = -1 means "wait forever" on the Rust side.
        let reader = scope.spawn(|| read(a_ref, payload.len() as u32, -1));
        std::thread::sleep(Duration::from_millis(300));
        let write_result = write(b_ref, payload);
        assert_eq!(write_result.error_code, 0);
        let read_result = reader.join().unwrap();
        assert_eq!(read_result.error_code, 0);
        assert_eq!(read_result.data, payload);
    });

    assert_eq!(close(a).error_code, 0);
    assert_eq!(close(b).error_code, 0);
}

fn drain_completes_without_error(path_a: &str, path_b: &str) {
    // Peer opened (and drained via read_exactly) so this doesn't leave
    // unread bytes sitting in the virtual driver's buffer for later
    // scenarios to trip over.
    let a = open(path_a, default_config()).expect("open a");
    let b = open(path_b, default_config()).expect("open b");
    let a_ref = unsafe { &*a };
    let b_ref = unsafe { &*b };

    let payload = b"flush me";
    assert_eq!(write(a_ref, payload).error_code, 0);
    assert_eq!(drain(a_ref).error_code, 0);
    assert_eq!(read_exactly(b_ref, payload.len(), Duration::from_secs(2)), payload);

    assert_eq!(close(a).error_code, 0);
    assert_eq!(close(b).error_code, 0);
}

fn configure_changes_baud_rate(path_a: &str, path_b: &str) {
    // Both ends must be open for this: writing into a com0com port with no
    // reader on the other side can block once its (small) internal buffer
    // has no room to drain into — that's a virtual-driver backpressure
    // characteristic, not something `configure`/`write` themselves do.
    let a = open(path_a, default_config()).expect("open a");
    let b = open(path_b, default_config()).expect("open b");
    let a_ref = unsafe { &*a };
    let b_ref = unsafe { &*b };

    let new_cfg = FfiSerialConfig { baud_rate: 9600, ..default_config() };
    assert_eq!(configure(a_ref, new_cfg).error_code, 0);

    // The port should still be fully usable after reconfiguring.
    let payload = b"still alive";
    assert_eq!(write(a_ref, payload).error_code, 0);
    let received = read_exactly(b_ref, payload.len(), Duration::from_secs(2));
    assert_eq!(received, payload);

    assert_eq!(close(a).error_code, 0);
    assert_eq!(close(b).error_code, 0);
}

fn close_releases_the_port_for_immediate_reopen(path_a: &str) {
    // Regression test: `close()` used to report completion before the OS
    // handle was actually released, so a same-port `open()` right after
    // could race into ERROR_ACCESS_DENIED.
    for _ in 0..5 {
        let a = open(path_a, default_config()).expect("open should succeed immediately after a prior close");
        assert_eq!(close(a).error_code, 0);
    }
}

fn opposite_direction_also_round_trips(path_a: &str, path_b: &str) {
    let a = open(path_a, default_config()).expect("open a");
    let b = open(path_b, default_config()).expect("open b");
    let a_ref = unsafe { &*a };
    let b_ref = unsafe { &*b };

    let payload = b"reverse-direction";
    assert_eq!(write(b_ref, payload).error_code, 0);
    let received = read_exactly(a_ref, payload.len(), Duration::from_secs(2));
    assert_eq!(received, payload);

    assert_eq!(close(a).error_code, 0);
    assert_eq!(close(b).error_code, 0);
}

fn repeated_write_read_cycles_do_not_leak_or_hang(path_a: &str, path_b: &str) {
    let a = open(path_a, default_config()).expect("open a");
    let b = open(path_b, default_config()).expect("open b");
    let a_ref = unsafe { &*a };
    let b_ref = unsafe { &*b };

    for i in 0..20 {
        let payload = format!("cycle-{i}");
        assert_eq!(write(a_ref, payload.as_bytes()).error_code, 0);
        let received = read_exactly(b_ref, payload.len(), Duration::from_secs(2));
        assert_eq!(received, payload.as_bytes(), "mismatch on cycle {i}");
    }

    assert_eq!(close(a).error_code, 0);
    assert_eq!(close(b).error_code, 0);
}

fn larger_payload_is_read_back_in_full(path_a: &str, path_b: &str) {
    let a = open(path_a, default_config()).expect("open a");
    let b = open(path_b, default_config()).expect("open b");
    let a_ref = unsafe { &*a };
    let b_ref = unsafe { &*b };

    // 8000 bytes almost certainly exceeds com0com's internal buffer, so
    // write() would block waiting for buffer space that only frees up if
    // something is reading concurrently. Reader and writer must run at the
    // same time here — awaiting the full write before starting the read
    // (like the other scenarios do for small payloads) would deadlock.
    let payload: Vec<u8> = (0..8000).map(|i| (i % 256) as u8).collect();
    std::thread::scope(|scope| {
        let reader = scope.spawn(|| read_exactly(b_ref, payload.len(), Duration::from_secs(5)));
        let write_result = write(a_ref, &payload);
        assert_eq!(write_result.error_code, 0);
        let received = reader.join().unwrap();
        assert_eq!(received, payload);
    });

    assert_eq!(close(a).error_code, 0);
    assert_eq!(close(b).error_code, 0);
}

fn opening_a_nonexistent_port_reports_an_error() {
    // Doesn't touch the loopback pair; just needs a COM port number that
    // (virtually certainly) doesn't exist.
    let result = open("COM250", default_config());
    assert!(result.is_err(), "expected opening a nonexistent port to fail");
}
