import 'dart:async';
import 'dart:convert';
import 'dart:ffi';
import 'dart:typed_data';

import 'package:ffi/ffi.dart';

import 'exceptions.dart';
import 'models.dart';
import 'native_bindings.dart';

class _CompletionResult {
  const _CompletionResult(this.errorCode, this.data, this.value);
  final int errorCode;
  final Uint8List data;
  final int value;
}

final Map<int, Completer<_CompletionResult>> _pending = <int, Completer<_CompletionResult>>{};
int _nextReqId = 0;

int _allocReqId() => _nextReqId++;

void _onCompletion(int reqId, int op, int errorCode, Pointer<Uint8> data, int dataLen, int value) {
  final completer = _pending.remove(reqId);
  if (completer == null) return;

  final bytes = dataLen > 0 ? Uint8List.fromList(data.asTypedList(dataLen)) : Uint8List(0);
  if (dataLen > 0) {
    aswFreeBuffer(data, dataLen);
  }
  completer.complete(_CompletionResult(errorCode, bytes, value));
}

/// Created lazily and kept alive for the process lifetime: every open port
/// shares this single listener, which is safe to invoke from any native
/// thread (the Rust reader/writer worker threads) and posts back onto this
/// isolate's event loop.
NativeCallable<CompletionCallbackNative>? _callable;

Pointer<NativeFunction<CompletionCallbackNative>> _ensureCallable() {
  final callable = _callable ??= NativeCallable<CompletionCallbackNative>.listener(_onCompletion);
  return callable.nativeFunction;
}

Future<_CompletionResult> _submit(
  void Function(int reqId, Pointer<NativeFunction<CompletionCallbackNative>> callback) invoke,
) {
  final reqId = _allocReqId();
  final completer = Completer<_CompletionResult>();
  _pending[reqId] = completer;
  invoke(reqId, _ensureCallable());
  return completer.future;
}

/// A non-blocking, overlapped-I/O-backed handle to a Windows serial port.
///
/// All native work (open, read, write, drain, configure, close) happens on
/// dedicated worker threads inside the Rust native library; nothing here
/// blocks the Dart isolate or polls in a loop.
final class WindowsSerialPort {
  WindowsSerialPort._(this._handle);

  Pointer<Void> _handle;
  bool _closed = false;

  static Future<List<SerialPortInfo>> list() async {
    final ptr = aswListPorts();
    try {
      final decoded = jsonDecode(ptr.toDartString()) as List<dynamic>;
      return decoded.map((entry) {
        final map = entry as Map<String, dynamic>;
        return SerialPortInfo(path: map['path'] as String, name: map['name'] as String?);
      }).toList(growable: false);
    } finally {
      aswFreeString(ptr);
    }
  }

  static Future<WindowsSerialPort> open(String path, SerialConfig config) async {
    final pathPtr = path.toNativeUtf8();
    final _CompletionResult result;
    try {
      result = await _submit(
        (reqId, callback) => aswOpen(
          pathPtr,
          config.baudRate,
          config.dataBits,
          config.stopBits.wireValue,
          config.parity.wireValue,
          config.flowControl.wireValue,
          reqId,
          callback,
        ),
      );
    } finally {
      calloc.free(pathPtr);
    }

    if (result.errorCode != 0) {
      throw SerialException('Failed to open $path', errorCode: result.errorCode);
    }
    return WindowsSerialPort._(Pointer<Void>.fromAddress(result.value));
  }

  /// Returns as soon as at least one byte has arrived, or an empty
  /// [Uint8List] if [timeout] elapses first. A `null` timeout waits
  /// indefinitely.
  Future<Uint8List> read(int maximumBytes, {Duration? timeout}) async {
    _checkOpen();
    final timeoutMs = timeout == null ? -1 : timeout.inMilliseconds;
    final result = await _submit(
      (reqId, callback) => aswRead(_handle, maximumBytes, timeoutMs, reqId, callback),
    );
    if (result.errorCode != 0) {
      throw SerialException('Read failed', errorCode: result.errorCode);
    }
    return result.data;
  }

  /// Completes once all of [bytes] has been accepted by `WriteFile`/the
  /// driver. Does not wait for physical transmission — see [drain].
  Future<void> write(Uint8List bytes) async {
    _checkOpen();
    final len = bytes.length;
    final buf = calloc<Uint8>(len == 0 ? 1 : len);
    if (len > 0) {
      buf.asTypedList(len).setAll(0, bytes);
    }
    final _CompletionResult result;
    try {
      result = await _submit((reqId, callback) => aswWrite(_handle, buf, len, reqId, callback));
    } finally {
      // Safe to free immediately: `asw_write` copies the buffer into an
      // owned Vec synchronously before this call returns.
      calloc.free(buf);
    }
    if (result.errorCode != 0) {
      throw SerialException('Write failed', errorCode: result.errorCode);
    }
  }

  /// Waits until previously written output has been physically transmitted
  /// (`FlushFileBuffers`). Separate from [write] because this can block for
  /// longer under flow control.
  Future<void> drain() async {
    _checkOpen();
    final result = await _submit((reqId, callback) => aswDrain(_handle, reqId, callback));
    if (result.errorCode != 0) {
      throw SerialException('Drain failed', errorCode: result.errorCode);
    }
  }

  Future<void> configure(SerialConfig config) async {
    _checkOpen();
    final result = await _submit(
      (reqId, callback) => aswConfigure(
        _handle,
        config.baudRate,
        config.dataBits,
        config.stopBits.wireValue,
        config.parity.wireValue,
        config.flowControl.wireValue,
        reqId,
        callback,
      ),
    );
    if (result.errorCode != 0) {
      throw SerialException('Configure failed', errorCode: result.errorCode);
    }
  }

  Future<void> close() async {
    if (_closed) return;
    _closed = true;
    final result = await _submit((reqId, callback) => aswClose(_handle, reqId, callback));
    if (result.errorCode != 0) {
      throw SerialException('Close failed', errorCode: result.errorCode);
    }
  }

  void _checkOpen() {
    if (_closed) {
      throw const SerialException('Port is closed', errorCode: errorCodePortClosed);
    }
  }
}
