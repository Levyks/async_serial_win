// Demonstrates SerialException and the read-timeout contract: a timed-out
// read returns an empty buffer rather than throwing, while real failures
// (port doesn't exist, access denied, device removed, etc.) throw
// SerialException carrying the underlying Win32 error code.

import 'package:async_serial_win/async_serial_win.dart';

void main() async {
  // Opening a port that doesn't exist throws SerialException, not a
  // generic FFI/OS error — the Win32 error code is preserved on it.
  try {
    await WindowsSerialPort.open('COM250', const SerialConfig(baudRate: 9600));
  } on SerialException catch (e) {
    print('Expected failure opening a nonexistent port:');
    print('  errorCode: ${e.errorCode}');
    print('  message:   ${e.message}');
  }

  final ports = await WindowsSerialPort.list();
  if (ports.isEmpty) {
    print('\nNo real ports available to demonstrate the timeout contract on.');
    return;
  }

  final port = await WindowsSerialPort.open(ports.first.path, const SerialConfig(baudRate: 115200));
  try {
    // A read timeout is not an error: it resolves normally with an empty
    // buffer, so you don't need try/catch just to handle "no data yet".
    final result = await port.read(64, timeout: const Duration(milliseconds: 300));
    print('\nTimed-out read returned ${result.length} bytes (no exception thrown).');

    // Using the port after close() throws SerialException(errorCodePortClosed).
    await port.close();
    try {
      await port.read(1);
    } on SerialException catch (e) {
      print('Using a closed port throws SerialException(errorCode: ${e.errorCode}).');
    }
  } finally {
    // close() is idempotent — safe to call again even if already closed above.
    await port.close();
  }
}
