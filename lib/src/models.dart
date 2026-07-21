/// A serial port discovered on the system.
final class SerialPortInfo {
  const SerialPortInfo({required this.path, this.name});

  /// The path Windows uses to open the port, e.g. `COM3`.
  final String path;

  /// The registry value name for this port, if available (usually equal to
  /// [path]). `null` if not reported by the enumeration source.
  final String? name;

  @override
  String toString() => 'SerialPortInfo(path: $path, name: $name)';
}

enum SerialStopBits {
  one(0),
  onePointFive(1),
  two(2);

  const SerialStopBits(this.wireValue);
  final int wireValue;
}

enum SerialParity {
  none(0),
  odd(1),
  even(2),
  mark(3),
  space(4);

  const SerialParity(this.wireValue);
  final int wireValue;
}

enum SerialFlowControl {
  none(0),
  rtsCts(1),
  xonXoff(2);

  const SerialFlowControl(this.wireValue);
  final int wireValue;
}

/// Serial line configuration, both for [WindowsSerialPort.open] and
/// [WindowsSerialPort.configure].
final class SerialConfig {
  const SerialConfig({
    required this.baudRate,
    this.dataBits = 8,
    this.stopBits = SerialStopBits.one,
    this.parity = SerialParity.none,
    this.flowControl = SerialFlowControl.none,
  });

  final int baudRate;
  final int dataBits;
  final SerialStopBits stopBits;
  final SerialParity parity;
  final SerialFlowControl flowControl;
}
