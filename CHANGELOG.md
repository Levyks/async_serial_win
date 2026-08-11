## 0.1.3

- Widen `hooks` to `>=1.0.0 <3.0.0` and `code_assets` to `>=1.0.0 <2.0.0` (previously `^2.0.2`/`^1.2.1`), so this package no longer forces a `hooks: ^2.x` constraint on projects that also depend on other packages still pinned to `hooks: ^1.x`. Verified against the oldest allowed pairing (`hooks 1.0.0` + `code_assets 1.0.0`, the lowest versions matching this package's `sdk: ^3.9.0` floor) — `code_assets >=1.2.0` requires `hooks ^2.0.0` internally, so both constraints had to widen together.

## 0.1.2

- Explicitly declare `platforms: windows` in `pubspec.yaml` so pub.dev shows this as a Windows-only package instead of inferring support for Android/iOS/Linux/macOS from static analysis.

## 0.1.1

- Add MIT license.
- Add more examples (basic usage, concurrent read/write, error handling).
- Add pub.dev and GitHub badges to the README.

## 0.1.0

- Initial release: `list`, `open`, `read` (with timeout), `write`, `drain`, `configure`, `close`.
- Overlapped Win32 I/O via a Rust native library, one reader/writer thread pair per open port.
- Prebuilt DLLs for x64, x86 (ia32), and ARM64 shipped via Dart native assets — no Rust toolchain required to consume the package.
