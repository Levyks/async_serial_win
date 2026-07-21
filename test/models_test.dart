import 'package:async_serial_win/async_serial_win.dart';
import 'package:test/test.dart';

/// Pure unit tests — no hardware/COM ports involved. Wire-value encodings
/// here must stay in sync with `rust/src/config.rs`'s `apply_to_dcb` match
/// arms; these tests pin the Dart side of that contract.
void main() {
  group('SerialConfig defaults', () {
    test('uses 8N1, no flow control, given only a baud rate', () {
      const config = SerialConfig(baudRate: 9600);
      expect(config.baudRate, 9600);
      expect(config.dataBits, 8);
      expect(config.stopBits, SerialStopBits.one);
      expect(config.parity, SerialParity.none);
      expect(config.flowControl, SerialFlowControl.none);
    });
  });

  group('wire value encodings', () {
    test('SerialStopBits', () {
      expect(SerialStopBits.one.wireValue, 0);
      expect(SerialStopBits.onePointFive.wireValue, 1);
      expect(SerialStopBits.two.wireValue, 2);
    });

    test('SerialParity', () {
      expect(SerialParity.none.wireValue, 0);
      expect(SerialParity.odd.wireValue, 1);
      expect(SerialParity.even.wireValue, 2);
      expect(SerialParity.mark.wireValue, 3);
      expect(SerialParity.space.wireValue, 4);
    });

    test('SerialFlowControl', () {
      expect(SerialFlowControl.none.wireValue, 0);
      expect(SerialFlowControl.rtsCts.wireValue, 1);
      expect(SerialFlowControl.xonXoff.wireValue, 2);
    });
  });

  group('SerialPortInfo', () {
    test('toString includes path and name', () {
      const info = SerialPortInfo(path: 'COM3', name: r'\Device\com0com10');
      expect(info.toString(), contains('COM3'));
      expect(info.toString(), contains(r'\Device\com0com10'));
    });
  });

  group('SerialException', () {
    test('toString includes the error code and message', () {
      const exception = SerialException('boom', errorCode: 5);
      expect(exception.toString(), contains('5'));
      expect(exception.toString(), contains('boom'));
    });

    test('errorCodePortClosed is a negative sentinel distinct from Win32 codes', () {
      expect(errorCodePortClosed, lessThan(0));
    });
  });
}
