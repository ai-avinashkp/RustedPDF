# Packaging RustedPdf for Windows

RustedPdf is **not a single-file app**. It binds to PDFium at runtime and looks
for the library next to its own executable, so `pdfium.dll` has to travel with
`RustedPdf.exe`. Ship the exe alone and the app starts, shows the empty state,
and then reports that PDFium could not be loaded.

That is the whole reason to build an installer rather than hand someone an exe.

## Build the MSI

```sh
cargo wix
```

Output: `target/wix/rusted_pdf-<version>-x86_64.msi` (~7.5 MB). The command
builds `--release` first, and `build.rs` copies `vendor/pdfium/pdfium.dll` next
to the binary, which is where [wix/main.wxs](wix/main.wxs) picks it up.

Requires [WiX Toolset v3](https://wixtoolset.org/) and `cargo install cargo-wix`.

## What gets installed

```
C:\Program Files\RustedPdf\
├── PDFIUM-LICENSE.txt
└── bin\
    ├── RustedPdf.exe
    └── pdfium.dll        ← same folder as the exe, by design
```

Plus:

* a **Start menu** shortcut,
* an **Add/Remove Programs** entry (proper uninstall),
* `SOFTWARE\Classes\Applications\RustedPdf.exe` and
  `.pdf\OpenWithList\RustedPdf.exe` — RustedPdf appears in the **Open with**
  menu for PDFs machine-wide. It does *not* become the default handler; that
  stays the user's choice.
* optionally the `bin` folder on `PATH` — a feature the user can decline in the
  installer's feature tree.

Uninstalling removes all of the above, **and** the per-user registration that
Settings ▸ *File manager* may have written. That second one is the awkward
case: it lives in `HKEY_CURRENT_USER`, which a per-machine uninstall running as
SYSTEM cannot see, so it would otherwise survive as a RustedPdf entry in
Explorer's *Open with* menu pointing at an executable that no longer exists.
The installer handles it with a deferred, impersonated custom action that runs

```
RustedPdf.exe --unregister-shell
```

before the files are deleted — the app undoing exactly what it wrote. It is
scheduled `Return='ignore'`, so it can never be the reason an uninstall fails,
and skipped when `UPGRADINGPRODUCTCODE` is set, so installing a new version
doesn't quietly strip a registration the user asked for.

## Verifying a build

An administrative install unpacks the payload without installing anything:

```powershell
msiexec /a target\wix\rusted_pdf-0.1.0-x86_64.msi /qn TARGETDIR=C:\Temp\check
dir /s C:\Temp\check
```

Expect `bin\RustedPdf.exe` and `bin\pdfium.dll` side by side.

## Two kinds of "Open with"

They coexist and are independent:

| | Installer (machine-wide) | Settings ▸ File manager (per user) |
| --- | --- | --- |
| Written to | `HKLM` | `HKCU` |
| Needs admin | yes (the install does) | no |
| Removed by | uninstalling | the *Remove* button |

The in-app registration exists for people running a build that was never
installed — a `cargo build --release` binary, or a copied folder.

## Releasing a new version

```sh
# 1. bump the version — this is what makes an upgrade an upgrade
#    Cargo.toml:  version = "0.3.0"
cargo wix
```

`UpgradeCode` stays fixed, so installing the new MSI **replaces** the old
install rather than sitting beside it in Add/Remove Programs. Windows compares
`ProductVersion`, which comes from `Cargo.toml`, so the version must go up —
the installer refuses downgrades with a clear message.

## The icon

[assets/icon.ico](assets/icon.ico) is a multi-size icon (16 → 256px) matching
the mark the titlebar draws: an accent-red rounded square, a white "R", and a
highlighter stroke that only appears at 32px and above, where it still reads.
It's used in three places:

* **the executable** — embedded by `build.rs` via `winresource`, along with the
  version metadata Explorer shows in Properties ▸ Details;
* **the installer** — `ARPPRODUCTICON` (Add/Remove Programs) and the Start menu
  shortcut;
* **the file type** — `DefaultIcon` on the registered application, so the
  "Open with" entry isn't a blank page.

`assets/icon.png` is the same artwork at 256px; the Linux integration writes it
into `~/.local/share/icons/hicolor/256x256/apps/` when you register.

Both are generated, not hand-drawn — regenerate them by re-running the script in
the commit that added them if the accent colour ever changes.

## Notes

* **The build is unsigned.** SmartScreen will warn on first run whether you
  ship an exe, a zip or an MSI; only a code-signing certificate changes that.
* **Settings are not removed on uninstall.** `%APPDATA%\RustedPdf\` keeps
  `settings.json`, `recent.json` and any crash journal. That is deliberate —
  reinstalling restores your preferences — but delete it by hand for a clean
  slate. The *Open with* entries are not in this category and are all removed.
* **`--unregister-shell` and `--register-shell`** do one job and exit without
  opening a window. They exist for the uninstaller, but they also work by hand
  on a portable copy, where there is no installer to do it for you.
* **`UpgradeCode` must never change** (`0B7234BF-…` in `wix/main.wxs`). It is
  what lets a new MSI upgrade an existing install instead of sitting beside it.
  Bump `version` in `Cargo.toml` for each release; the installer refuses
  downgrades.
* **The ICE69 warnings** from `light.exe` are expected: the shortcut and the
  registry entry reference `[#exe0]` from a sibling component in the same
  feature, which is fine because they always install together.

## Portable alternative

If you just want to hand someone a folder, zip these two files together:

```
RustedPdf.exe
pdfium.dll
```

Both from `target/release/`. It runs from anywhere, needs no admin, and stores
its settings in `%APPDATA%\RustedPdf`. Use Settings ▸ File manager if you want
the *Open with* entry. Keep the two files together — that is the only rule.
