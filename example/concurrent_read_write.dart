// Demonstrates that read() and write() can be genuinely in flight at the
// same time — a read can be pending (blocked, waiting for data on a
// background thread) while a write happens concurrently on the same port.
//
// Run with two ports wired to each other (e.g. a com0com pair, or two ends
// of a real null-modem cable):
//   dart run example/concurrent_read_write.dart COM6 COM7

import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

import 'package:async_serial_win/async_serial_win.dart';

void main(List<String> args) async {
  if (args.length < 2) {
    stderr.writeln('Usage: dart run example/concurrent_read_write.dart <portA> <portB>');
    return;
  }

  const config = SerialConfig(baudRate: 115200);
  final a = await WindowsSerialPort.open(args[0], config);
  final b = await WindowsSerialPort.open(args[1], config);

  try {
    // Start a read on `a` before any data exists yet — this genuinely
    // blocks a background thread waiting for bytes, it does not poll.
    final readFuture = a.read(64, timeout: const Duration(seconds: 5));
    print('Read pending on ${args[0]}...');

    await Future<void>.delayed(const Duration(milliseconds: 200));

    // The write on `b` runs while the read above is still pending.
    await b.write(Uint8List.fromList(utf8.encode('hello from ${args[1]}')));
    print('Wrote from ${args[1]} while the read was still pending.');

    // Waiting for physical transmission is a separate, explicit step —
    // write() only guarantees the driver accepted the bytes.
    await b.drain();

    final received = await readFuture;
    print('Read completed: ${utf8.decode(received)}');
  } finally {
    await a.close();
    await b.close();
  }
}
