/// Test-only helpers shared between `example/` scripts and the `test/`
/// suite. Not exported from `async_serial_win.dart` — this is implementation
/// detail for this package's own testing, not part of the public API.
library;

import 'dart:async';
import 'dart:convert';

import '../async_serial_win.dart';

/// Reads until [expectedLength] bytes have been collected or [overallTimeout]
/// elapses. A single `read()` call may legitimately return as soon as the
/// first byte arrives (per its documented semantics) without waiting for the
/// rest of a chunked/relayed write, so accumulating an exact expected length
/// needs to happen across possibly-multiple reads.
Future<List<int>> readExactly(WindowsSerialPort port, int expectedLength, Duration overallTimeout) async {
  final deadline = DateTime.now().add(overallTimeout);
  final bytes = <int>[];
  while (bytes.length < expectedLength) {
    final remaining = deadline.difference(DateTime.now());
    if (remaining <= Duration.zero) break;
    final chunk = await port.read(expectedLength - bytes.length, timeout: remaining);
    if (chunk.isEmpty) break;
    bytes.addAll(chunk);
  }
  return bytes;
}

/// Finds a working virtual loopback pair (e.g. a com0com pair) by probing
/// unordered pairs of ports: open both once, try both write directions
/// without reopening in between, and see if a marker round-trips. This makes
/// tests portable across machines instead of hardcoding port names.
///
/// Throws [StateError] if no pair is found — these tests assume a virtual
/// null-modem pair is installed on the machine running them.
Future<(String, String)> findLoopbackPair() async {
  final ports = await WindowsSerialPort.list();
  const probeConfig = SerialConfig(baudRate: 115200);
  const marker = 'async_serial_win-probe';

  for (var i = 0; i < ports.length; i++) {
    for (var j = i + 1; j < ports.length; j++) {
      final pathA = ports[i].path;
      final pathB = ports[j].path;

      WindowsSerialPort? a;
      WindowsSerialPort? b;
      try {
        a = await WindowsSerialPort.open(pathA, probeConfig).timeout(const Duration(seconds: 2));
        b = await WindowsSerialPort.open(pathB, probeConfig).timeout(const Duration(seconds: 2));

        for (final (writer, reader, writerPath, readerPath) in [
          (a, b, pathA, pathB),
          (b, a, pathB, pathA),
        ]) {
          final readFuture = readExactly(reader, marker.length, const Duration(seconds: 1));
          try {
            await writer.write(utf8.encode(marker)).timeout(const Duration(seconds: 1));
          } catch (_) {
            continue;
          }
          if (utf8.decode(await readFuture, allowMalformed: true) == marker) {
            return (writerPath, readerPath);
          }
        }
      } catch (_) {
        // Not a usable/paired combination (or in use) — try the next one.
      } finally {
        await a?.close().timeout(const Duration(seconds: 2), onTimeout: () {});
        await b?.close().timeout(const Duration(seconds: 2), onTimeout: () {});
      }
    }
  }

  throw StateError(
    'No loopback pair found among ${ports.map((p) => p.path).join(', ')}. '
    'Install a virtual null-modem pair (e.g. com0com) to run these tests.',
  );
}
