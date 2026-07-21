import 'dart:convert';
import 'dart:typed_data';

import 'package:async_serial_win/async_serial_win.dart';
import 'package:async_serial_win/src/testing.dart';

/// Demonstrates the library end-to-end against an auto-discovered loopback
/// pair. For the actual test suite, see `test/serial_port_test.dart`.
void main() async {
  final (aPath, bPath) = await findLoopbackPair();
  print('Using loopback pair: $aPath <-> $bPath');

  const config = SerialConfig(baudRate: 115200);
  final a = await WindowsSerialPort.open(aPath, config);
  final b = await WindowsSerialPort.open(bPath, config);

  // Concurrent read (pending before data arrives) + write, per the required
  // semantics: read() must be able to be pending while write() runs.
  final readFuture = a.read(64, timeout: const Duration(seconds: 3));
  await Future<void>.delayed(const Duration(milliseconds: 200));
  await b.write(Uint8List.fromList(utf8.encode('hello overlapped world')));
  await b.drain();

  final received = await readFuture;
  print('Received: ${utf8.decode(received)}');

  // Timeout with no data should yield an empty buffer, not hang or throw.
  final sw = Stopwatch()..start();
  final empty = await a.read(64, timeout: const Duration(milliseconds: 500));
  sw.stop();
  print('Timeout read returned ${empty.length} bytes after ${sw.elapsedMilliseconds}ms');

  await a.configure(const SerialConfig(baudRate: 9600));
  print('Reconfigured $aPath to 9600 baud OK');

  await a.close();
  await b.close();
  print('Closed both ports OK');
}
