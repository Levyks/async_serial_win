/// Thrown when a native serial operation fails.
///
/// [errorCode] is the raw Win32 error code (e.g. from `GetLastError`), or a
/// negative sentinel for errors detected purely on the Dart side.
final class SerialException implements Exception {
  const SerialException(this.message, {required this.errorCode});

  final String message;
  final int errorCode;

  @override
  String toString() => 'SerialException($errorCode): $message';
}

const int errorCodePortClosed = -1;
