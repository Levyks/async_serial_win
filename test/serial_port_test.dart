import 'dart:async';
import 'dart:convert';
import 'dart:typed_data';

import 'package:async_serial_win/async_serial_win.dart';
import 'package:async_serial_win/src/testing.dart';
import 'package:test/test.dart';

/// Hardware-in-the-loop tests. These require a virtual null-modem pair
/// (e.g. com0com) installed on the machine running `dart test` —
/// `findLoopbackPair()` locates it by brute-force probing rather than
/// hardcoding port names, so this works across machines.
///
/// `package:test` runs tests within a single file sequentially by default,
/// which is required here: every test needs exclusive use of the same two
/// physical/virtual COM ports, so concurrent execution would just race tests
/// against each other for the same hardware.
void main() {
  late String pathA;
  late String pathB;

  setUpAll(() async {
    final pair = await findLoopbackPair();
    pathA = pair.$1;
    pathB = pair.$2;
    printOnFailure('Using loopback pair: $pathA <-> $pathB');
  });

  const config = SerialConfig(baudRate: 115200);

  test('list() finds at least the loopback pair', () async {
    final ports = await WindowsSerialPort.list();
    final paths = ports.map((p) => p.path).toSet();
    expect(paths, containsAll([pathA, pathB]));
  });

  test('basic write then read round-trips the exact bytes', () async {
    final a = await WindowsSerialPort.open(pathA, config);
    final b = await WindowsSerialPort.open(pathB, config);
    addTearDown(() async {
      await a.close();
      await b.close();
    });

    final payload = Uint8List.fromList(utf8.encode('hello overlapped world'));
    await a.write(payload);
    final received = await readExactly(b, payload.length, const Duration(seconds: 2));
    expect(received, payload);
  });

  test('read() can be pending while write() is in flight', () async {
    final a = await WindowsSerialPort.open(pathA, config);
    final b = await WindowsSerialPort.open(pathB, config);
    addTearDown(() async {
      await a.close();
      await b.close();
    });

    final payload = utf8.encode('concurrent-probe');
    final readFuture = a.read(payload.length, timeout: const Duration(seconds: 3));
    await Future<void>.delayed(const Duration(milliseconds: 150));
    await b.write(Uint8List.fromList(payload));

    expect(await readFuture, payload);
  });

  test('read() with a timeout and no data returns an empty buffer without hanging', () async {
    final a = await WindowsSerialPort.open(pathA, config);
    addTearDown(() => a.close());

    final stopwatch = Stopwatch()..start();
    final result = await a.read(64, timeout: const Duration(milliseconds: 500));
    stopwatch.stop();

    expect(result, isEmpty);
    expect(stopwatch.elapsedMilliseconds, greaterThanOrEqualTo(400));
    expect(stopwatch.elapsedMilliseconds, lessThan(3000));
  });

  test('read() with a zero timeout polls without blocking', () async {
    final a = await WindowsSerialPort.open(pathA, config);
    addTearDown(() => a.close());

    final stopwatch = Stopwatch()..start();
    final result = await a.read(64, timeout: Duration.zero);
    stopwatch.stop();

    expect(result, isEmpty);
    expect(stopwatch.elapsedMilliseconds, lessThan(300));
  });

  test('read() with no timeout blocks until data actually arrives', () async {
    final a = await WindowsSerialPort.open(pathA, config);
    final b = await WindowsSerialPort.open(pathB, config);
    addTearDown(() async {
      await a.close();
      await b.close();
    });

    final payload = utf8.encode('no-timeout-probe');
    final readFuture = a.read(payload.length); // no timeout => waits forever
    await Future<void>.delayed(const Duration(milliseconds: 300));
    await b.write(Uint8List.fromList(payload));

    expect(await readFuture, payload);
  });

  test('drain() completes without error after a write', () async {
    final a = await WindowsSerialPort.open(pathA, config);
    final b = await WindowsSerialPort.open(pathB, config);
    addTearDown(() async {
      await a.close();
      await b.close();
    });

    final payload = utf8.encode('flush me');
    await a.write(Uint8List.fromList(payload));
    await a.drain();
    // Peer drains the bytes so they don't linger for later tests.
    expect(await readExactly(b, payload.length, const Duration(seconds: 2)), payload);
  });

  test('configure() changes the baud rate and the port stays usable', () async {
    final a = await WindowsSerialPort.open(pathA, config);
    final b = await WindowsSerialPort.open(pathB, config);
    addTearDown(() async {
      await a.close();
      await b.close();
    });

    await a.configure(const SerialConfig(baudRate: 9600));

    final payload = utf8.encode('still alive');
    await a.write(Uint8List.fromList(payload));
    expect(await readExactly(b, payload.length, const Duration(seconds: 2)), payload);
  });

  test('close() releases the OS handle so the same port can be reopened immediately', () async {
    // Regression test: close() used to report completion before the OS
    // handle was actually released, letting a same-port open() race into
    // ERROR_ACCESS_DENIED.
    for (var i = 0; i < 5; i++) {
      final a = await WindowsSerialPort.open(pathA, config);
      await a.close();
    }
  });

  test('methods throw SerialException after close()', () async {
    final a = await WindowsSerialPort.open(pathA, config);
    await a.close();

    expect(() => a.read(1), throwsA(isA<SerialException>()));
    expect(() => a.write(Uint8List(1)), throwsA(isA<SerialException>()));
    expect(() => a.drain(), throwsA(isA<SerialException>()));
    expect(() => a.configure(config), throwsA(isA<SerialException>()));
  });

  test('close() is idempotent', () async {
    final a = await WindowsSerialPort.open(pathA, config);
    await a.close();
    await a.close(); // should not throw or hang
  });

  test('opening a nonexistent port throws with a Win32 error code', () async {
    await expectLater(
      WindowsSerialPort.open('COM250', config),
      throwsA(isA<SerialException>().having((e) => e.errorCode, 'errorCode', greaterThan(0))),
    );
  });

  test('the reverse direction also round-trips', () async {
    final a = await WindowsSerialPort.open(pathA, config);
    final b = await WindowsSerialPort.open(pathB, config);
    addTearDown(() async {
      await a.close();
      await b.close();
    });

    final payload = utf8.encode('reverse-direction');
    await b.write(Uint8List.fromList(payload));
    expect(await readExactly(a, payload.length, const Duration(seconds: 2)), payload);
  });

  test('repeated write/read cycles do not leak or hang', () async {
    final a = await WindowsSerialPort.open(pathA, config);
    final b = await WindowsSerialPort.open(pathB, config);
    addTearDown(() async {
      await a.close();
      await b.close();
    });

    for (var i = 0; i < 20; i++) {
      final payload = utf8.encode('cycle-$i');
      await a.write(Uint8List.fromList(payload));
      expect(await readExactly(b, payload.length, const Duration(seconds: 2)), payload, reason: 'cycle $i');
    }
  });

  test('a larger payload exceeding the driver buffer is read back in full', () async {
    final a = await WindowsSerialPort.open(pathA, config);
    final b = await WindowsSerialPort.open(pathB, config);
    addTearDown(() async {
      await a.close();
      await b.close();
    });

    // Large enough to likely exceed com0com's internal buffer, so the
    // reader must be draining concurrently with the write, not after it.
    final payload = Uint8List.fromList(List.generate(8000, (i) => i % 256));
    final readFuture = readExactly(b, payload.length, const Duration(seconds: 5));
    await a.write(payload);
    expect(await readFuture, payload);
  });
}
