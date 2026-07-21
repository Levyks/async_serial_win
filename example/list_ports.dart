import 'package:async_serial_win/async_serial_win.dart';

void main() async {
  final ports = await WindowsSerialPort.list();
  if (ports.isEmpty) {
    print('No serial ports found.');
  }
  for (final port in ports) {
    print('${port.path} (${port.name})');
  }
}
