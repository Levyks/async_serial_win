// The canonical, minimal example: list ports, open one, write, read, close.
//
// Run with a real COM port name, e.g.:
//   dart run example/async_serial_win_example.dart COM3

import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

import 'package:async_serial_win/async_serial_win.dart';

void main(List<String> args) async {
  final ports = await WindowsSerialPort.list();
  print('Available ports:');
  for (final port in ports) {
    print('  ${port.path} (${port.name})');
  }

  final path = args.isNotEmpty ? args.first : (ports.isEmpty ? null : ports.first.path);
  if (path == null) {
    print('\nNo ports available and none specified. Pass a port name, e.g.:');
    print('  dart run example/async_serial_win_example.dart COM3');
    return;
  }

  print('\nOpening $path...');
  final port = await WindowsSerialPort.open(path, const SerialConfig(baudRate: 115200));

  try {
    await port.write(Uint8List.fromList(utf8.encode('AT\r\n')));
    print('Wrote "AT\\r\\n", waiting for a reply...');

    // Returns as soon as at least one byte arrives, or empty if nothing
    // shows up within the timeout — it never polls or busy-waits.
    final response = await port.read(256, timeout: const Duration(seconds: 2));
    if (response.isEmpty) {
      print('No response within timeout.');
    } else {
      print('Received: ${utf8.decode(response, allowMalformed: true)}');
    }
  } on SerialException catch (e) {
    stderr.writeln('Serial error (Win32 code ${e.errorCode}): ${e.message}');
  } finally {
    await port.close();
  }
}
