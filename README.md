# async_serial_win

[![pub package](https://img.shields.io/pub/v/async_serial_win.svg)](https://pub.dev/packages/async_serial_win)
[![GitHub](https://img.shields.io/badge/GitHub-Levyks%2Fasync__serial__win-181717?logo=github)](https://github.com/Levyks/async_serial_win)

Truly non-blocking, async serial port I/O for Dart and Flutter on Windows — backed by a small Rust native library that uses real overlapped Win32 I/O. No polling, no busy-wait loops, no blocking the isolate.

## Why this exists

Most serial port packages on pub.dev for Windows are thin wrappers around synchronous Win32 calls, occasionally hidden behind a `Timer.periodic` poll loop to fake asynchrony. That works, but it means:

- Reads either block a thread or burn CPU polling for data that isn't there yet.
- Writes and reads can't be genuinely in flight at the same time.
- "Wait for data or time out" ends up implemented as "check every N milliseconds," which adds latency and wastes cycles.

`async_serial_win` instead does it the way Windows actually wants you to: `CreateFileW` with `FILE_FLAG_OVERLAPPED`, `ReadFile`/`WriteFile` with real `OVERLAPPED` structures, and `WaitForMultipleObjects` on dedicated worker threads. A read genuinely blocks (on a background thread, never on your isolate) until at least one byte arrives or a timeout elapses — it does not spin. A write and a read can be pending at the same moment, because they run on separate reader/writer threads per port.

## How it's built

- **Rust** implements the native side (`rust/`) — one reader thread and one writer thread per open port, communicating with Dart via a small `extern "C"` callback ABI.
- **Prebuilt binaries, no toolchain required.** The three Windows target DLLs (x64, x86/ia32, ARM64) are committed under `rust/prebuilt/` and wired up through Dart's [native assets](https://dart.dev/interop/native-assets) build hook. You do not need Rust, Cargo, or Visual Studio installed to consume this package — `dart pub get` and you're done. (Windows on ARM32 isn't a target because Microsoft dropped it years ago; x64/x86/ARM64 is the complete modern set.)
- **Plain Dart package, not a Flutter plugin.** Works from a bare `dart run` CLI app just as well as from a Flutter Windows desktop app — no `windows/CMakeLists.txt`, no plugin registration.
- Every operation — open, read, write, drain, configure, close — is genuinely asynchronous and returns a `Future`. `read()` and `write()` can be pending concurrently; `write()`, `configure()`, and `close()` are internally serialized in call order.

## Requirements

- Windows (x64, x86, or ARM64).
- Dart SDK `^3.9.0` (for native assets support).

## Install

```yaml
dependencies:
  async_serial_win: ^0.1.0
```

## Quick start

```dart
import 'package:async_serial_win/async_serial_win.dart';

void main() async {
  // List available ports.
  final ports = await WindowsSerialPort.list();
  for (final port in ports) {
    print('${port.path} (${port.name})');
  }

  // Open one.
  final port = await WindowsSerialPort.open(
    'COM3',
    const SerialConfig(baudRate: 115200),
  );

  // Write — completes once the whole buffer is handed to the driver.
  await port.write(Uint8List.fromList('AT\r\n'.codeUnits));

  // Read — returns as soon as at least one byte arrives, or an empty
  // buffer if the timeout elapses first. This genuinely blocks a
  // background thread; it does not poll.
  final response = await port.read(64, timeout: const Duration(seconds: 2));
  print(String.fromCharCodes(response));

  await port.close();
}
```

## Concurrent read + write

Because reads and writes run on separate worker threads, you can have a read pending while a write happens — no need to serialize them yourself:

```dart
final readFuture = port.read(64, timeout: const Duration(seconds: 5));

// This can run while the read above is still waiting for data.
await port.write(Uint8List.fromList('PING'.codeUnits));

final reply = await readFuture;
```

## Waiting for physical transmission

`write()` only guarantees the driver has accepted the bytes. If you need to know the bytes have actually gone out on the wire (e.g. before toggling a control line under flow control), call `drain()` separately:

```dart
await port.write(payload);
await port.drain(); // waits for FlushFileBuffers to confirm transmission
```

## Reconfiguring a port

```dart
await port.configure(const SerialConfig(
  baudRate: 9600,
  dataBits: 8,
  stopBits: SerialStopBits.one,
  parity: SerialParity.even,
  flowControl: SerialFlowControl.rtsCts,
));
```

## Error handling

Failures surface as `SerialException`, carrying the underlying Win32 error code:

```dart
try {
  final port = await WindowsSerialPort.open('COM99', config);
} on SerialException catch (e) {
  print('Failed to open (Win32 error ${e.errorCode}): ${e.message}');
}
```

## A note on virtual/null-modem ports

If you're testing against a virtual loopback pair (e.g. [com0com](https://sourceforge.net/projects/com0com/)), keep in mind the virtual driver's internal buffer is small: writing a large payload with nobody actively reading on the other end can block until a reader drains it. This is a property of the driver, not the library — with real hardware or an actively-read peer it isn't an issue, but for loopback testing, start your read before (or concurrently with) a large write.

## API surface

```dart
class WindowsSerialPort {
  static Future<List<SerialPortInfo>> list();
  static Future<WindowsSerialPort> open(String path, SerialConfig config);

  Future<Uint8List> read(int maximumBytes, {Duration? timeout});
  Future<void> write(Uint8List bytes);
  Future<void> drain();
  Future<void> configure(SerialConfig config);
  Future<void> close();
}
```

## Examples

See `example/`:

- [`async_serial_win_example.dart`](example/async_serial_win_example.dart) — the minimal list/open/write/read/close flow.
- [`concurrent_read_write.dart`](example/concurrent_read_write.dart) — a pending `read()` and an in-flight `write()` on the same port at once.
- [`error_handling.dart`](example/error_handling.dart) — `SerialException`, the read-timeout-returns-empty contract, and using a port after `close()`.
- [`list_ports.dart`](example/list_ports.dart) — just enumerating available ports.
- [`loopback_test.dart`](example/loopback_test.dart) — a fuller end-to-end demo against an auto-discovered loopback pair.

`test/` doubles as detailed usage reference too, since it exercises every operation's documented semantics.

## Status

Functional and tested against real overlapped I/O (see `test/` and `rust/tests/`), but young — see [CHANGELOG](CHANGELOG.md) for what's covered. Issues and PRs welcome.
