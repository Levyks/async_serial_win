## 0.1.0

- Initial release: `list`, `open`, `read` (with timeout), `write`, `drain`, `configure`, `close`.
- Overlapped Win32 I/O via a Rust native library, one reader/writer thread pair per open port.
- Prebuilt DLLs for x64, x86 (ia32), and ARM64 shipped via Dart native assets — no Rust toolchain required to consume the package.
