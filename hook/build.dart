import 'dart:io';

import 'package:code_assets/code_assets.dart';
import 'package:hooks/hooks.dart';

/// Registers the prebuilt Rust native library (`rust/prebuilt/<triple>/`) as
/// a code asset. No Rust toolchain is required on consumers' machines — the
/// three supported target triples are built and committed ahead of time (see
/// `rust/build_prebuilt.ps1`).
void main(List<String> args) async {
  await build(args, (input, output) async {
    if (!input.config.buildCodeAssets) return;

    final targetOS = input.config.code.targetOS;
    if (targetOS != OS.windows) {
      // This package only supports Windows; silently skip elsewhere so it
      // doesn't break `dart pub get`/analysis on non-Windows machines.
      return;
    }

    final triple = _rustTriple(input.config.code.targetArchitecture);
    final dllFile = File.fromUri(
      input.packageRoot.resolve('rust/prebuilt/$triple/async_serial_win.dll'),
    );

    if (!dllFile.existsSync()) {
      throw Exception(
        'No prebuilt async_serial_win.dll for target $triple. '
        'Expected at: ${dllFile.path}',
      );
    }

    output.assets.code.add(
      CodeAsset(
        package: input.packageName,
        name: 'src/native_bindings.dart',
        linkMode: DynamicLoadingBundled(),
        file: dllFile.uri,
      ),
    );

    output.dependencies.add(dllFile.uri);
  });
}

String _rustTriple(Architecture architecture) {
  return switch (architecture) {
    Architecture.x64 => 'x86_64-pc-windows-msvc',
    Architecture.ia32 => 'i686-pc-windows-msvc',
    Architecture.arm64 => 'aarch64-pc-windows-msvc',
    _ => throw UnsupportedError('Unsupported architecture: $architecture'),
  };
}
