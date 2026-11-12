# Vendored PDFium

`pdfium-render` is a binding, not an engine — it needs PDFium itself at runtime.
The library here is a prebuilt binary from
[bblanchon/pdfium-binaries](https://github.com/bblanchon/pdfium-binaries)
(Apache-2.0 / BSD-3-Clause; see `LICENSE`), version in `VERSION`.

[build.rs](../../build.rs) copies it next to the executable on every build, and
[src/pdf/engine.rs](../../src/pdf/engine.rs) looks for it there first, then in
this directory, then in the system library path.

## Refreshing / other platforms

Download the archive for the target and drop the shared library in this folder:

| Platform | Asset | Library |
| --- | --- | --- |
| Windows x64 | `pdfium-win-x64.tgz` | `bin/pdfium.dll` → `pdfium.dll` |
| macOS arm64 | `pdfium-mac-arm64.tgz` | `lib/libpdfium.dylib` → `libpdfium.dylib` |
| macOS x64 | `pdfium-mac-x64.tgz` | `lib/libpdfium.dylib` → `libpdfium.dylib` |
| Linux x64 | `pdfium-linux-x64.tgz` | `lib/libpdfium.so` → `libpdfium.so` |

```sh
curl -L -o pdfium.tgz \
  https://github.com/bblanchon/pdfium-binaries/releases/latest/download/pdfium-win-x64.tgz
tar -xzf pdfium.tgz
```

The binary is intentionally not committed to source control if your repo has a
size policy — the build prints a warning and the app still starts (it just
can't open documents) when the file is absent.
