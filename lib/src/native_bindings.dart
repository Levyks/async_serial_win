// ignore_for_file: non_constant_identifier_names
//
// Raw FFI declarations for the Rust `async_serial_win` native library.
// The `@Native` annotations resolve against the code asset registered by
// `hook/build.dart` under the id `package:async_serial_win/src/native_bindings.dart`
// (native assets match the asset name to the declaring file's path), so this
// file must stay at `lib/src/native_bindings.dart`.

import 'dart:ffi';

import 'package:ffi/ffi.dart';

/// Matches the Rust `CompletionCallback` typedef in `rust/src/port.rs`.
///
/// `data`/`dataLen` describe a heap buffer that must be freed via
/// [aswFreeBuffer] after the bytes are copied out (only ever non-null for
/// `OP_READ`). `value` carries an inline payload (the opened port handle,
/// for `OP_OPEN`).
typedef CompletionCallbackNative =
    Void Function(
      Uint64 reqId,
      Int32 op,
      Int32 errorCode,
      Pointer<Uint8> data,
      Uint32 dataLen,
      Uint64 value,
    );

typedef CompletionCallbackDart =
    void Function(int reqId, int op, int errorCode, Pointer<Uint8> data, int dataLen, int value);

const int opOpen = 0;
const int opRead = 1;
const int opWrite = 2;
const int opDrain = 3;
const int opConfigure = 4;
const int opClose = 5;

@Native<Pointer<Utf8> Function()>(symbol: 'asw_list_ports')
external Pointer<Utf8> aswListPorts();

@Native<Void Function(Pointer<Utf8>)>(symbol: 'asw_free_string')
external void aswFreeString(Pointer<Utf8> ptr);

@Native<Void Function(Pointer<Uint8>, Uint32)>(symbol: 'asw_free_buffer')
external void aswFreeBuffer(Pointer<Uint8> ptr, int len);

@Native<
  Void Function(
    Pointer<Utf8> path,
    Uint32 baudRate,
    Uint8 dataBits,
    Uint8 stopBits,
    Uint8 parity,
    Uint8 flowControl,
    Uint64 reqId,
    Pointer<NativeFunction<CompletionCallbackNative>> callback,
  )
>(symbol: 'asw_open')
external void aswOpen(
  Pointer<Utf8> path,
  int baudRate,
  int dataBits,
  int stopBits,
  int parity,
  int flowControl,
  int reqId,
  Pointer<NativeFunction<CompletionCallbackNative>> callback,
);

@Native<
  Void Function(
    Pointer<Void> port,
    Uint32 maxBytes,
    Int64 timeoutMs,
    Uint64 reqId,
    Pointer<NativeFunction<CompletionCallbackNative>> callback,
  )
>(symbol: 'asw_read')
external void aswRead(
  Pointer<Void> port,
  int maxBytes,
  int timeoutMs,
  int reqId,
  Pointer<NativeFunction<CompletionCallbackNative>> callback,
);

@Native<
  Void Function(
    Pointer<Void> port,
    Pointer<Uint8> data,
    Uint32 len,
    Uint64 reqId,
    Pointer<NativeFunction<CompletionCallbackNative>> callback,
  )
>(symbol: 'asw_write')
external void aswWrite(
  Pointer<Void> port,
  Pointer<Uint8> data,
  int len,
  int reqId,
  Pointer<NativeFunction<CompletionCallbackNative>> callback,
);

@Native<
  Void Function(Pointer<Void> port, Uint64 reqId, Pointer<NativeFunction<CompletionCallbackNative>> callback)
>(symbol: 'asw_drain')
external void aswDrain(Pointer<Void> port, int reqId, Pointer<NativeFunction<CompletionCallbackNative>> callback);

@Native<
  Void Function(
    Pointer<Void> port,
    Uint32 baudRate,
    Uint8 dataBits,
    Uint8 stopBits,
    Uint8 parity,
    Uint8 flowControl,
    Uint64 reqId,
    Pointer<NativeFunction<CompletionCallbackNative>> callback,
  )
>(symbol: 'asw_configure')
external void aswConfigure(
  Pointer<Void> port,
  int baudRate,
  int dataBits,
  int stopBits,
  int parity,
  int flowControl,
  int reqId,
  Pointer<NativeFunction<CompletionCallbackNative>> callback,
);

@Native<
  Void Function(Pointer<Void> port, Uint64 reqId, Pointer<NativeFunction<CompletionCallbackNative>> callback)
>(symbol: 'asw_close')
external void aswClose(Pointer<Void> port, int reqId, Pointer<NativeFunction<CompletionCallbackNative>> callback);
