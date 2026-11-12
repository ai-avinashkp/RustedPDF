//! Desktop integration: making RustedPdf appear in the file manager's
//! "Open with" menu for PDFs.
//!
//! Registration is always **per-user** and additive — it never takes over as
//! the default PDF handler, and never touches machine-wide settings. Nothing
//! here needs administrator rights, and [`unregister`] undoes exactly what
//! [`register`] wrote.
//!
//! The app already opens whatever paths it's given on the command line (see
//! `main.rs`), so registration is only about telling the OS that.

use anyhow::Result;

/// What registering does on this platform, for the Settings screen.
pub fn describe() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "Adds RustedPdf to the \"Open with\" menu for .pdf files. Your default PDF \
         app is left as it is."
    }
    #[cfg(target_os = "linux")]
    {
        "Installs a desktop entry so file managers offer RustedPdf for PDFs."
    }
    #[cfg(target_os = "macos")]
    {
        "On macOS this is declared by the .app bundle itself — see the note below."
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use std::path::PathBuf;

    use anyhow::{anyhow, Context as _, Result};
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
    use winreg::RegKey;

    /// Our entry in the registry. Named so it can't collide with another app's.
    const PROGID: &str = "RustedPdf.Document";

    fn executable() -> Result<PathBuf> {
        std::env::current_exe().context("Could not find the RustedPdf executable")
    }

    fn classes() -> Result<RegKey> {
        RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey_with_flags("Software\\Classes", KEY_READ | KEY_WRITE)
            .context("Could not open the per-user file associations")
    }

    pub fn is_registered() -> bool {
        let Ok(classes) = classes() else {
            return false;
        };
        let Ok(command) = classes.open_subkey(format!("{PROGID}\\shell\\open\\command")) else {
            return false;
        };
        let Ok(registered) = command.get_value::<String, _>("") else {
            return false;
        };

        // A stale entry pointing at an executable that has since moved (or been
        // rebuilt elsewhere) is worse than none: report it as unregistered so
        // the user re-registers and fixes the path.
        executable()
            .map(|path| registered.contains(&path.to_string_lossy().to_string()))
            .unwrap_or(false)
    }

    pub fn register() -> Result<()> {
        let executable = executable()?;
        let executable = executable.to_string_lossy();
        let classes = classes()?;

        // The document type: what Windows shows and how it launches us.
        let (program, _) = classes.create_subkey(PROGID)?;
        program.set_value("", &"PDF Document")?;

        let (icon, _) = classes.create_subkey(format!("{PROGID}\\DefaultIcon"))?;
        icon.set_value("", &format!("{executable},0"))?;

        let (command, _) = classes.create_subkey(format!("{PROGID}\\shell\\open\\command"))?;
        command.set_value("", &format!("\"{executable}\" \"%1\""))?;

        // Listing the ProgID under the extension adds us to "Open with" without
        // claiming the default — that choice stays the user's.
        let (associations, _) = classes.create_subkey(".pdf\\OpenWithProgids")?;
        associations
            .set_value(PROGID, &"")
            .map_err(|error| anyhow!("Could not add RustedPdf to the Open with list: {error}"))?;

        refresh_shell();
        Ok(())
    }

    pub fn unregister() -> Result<()> {
        let classes = classes()?;

        if let Ok(associations) = classes.open_subkey_with_flags(".pdf\\OpenWithProgids", KEY_WRITE)
        {
            let _ = associations.delete_value(PROGID);
        }
        let _ = classes.delete_subkey_all(PROGID);

        refresh_shell();
        Ok(())
    }

    /// Tells Explorer to re-read associations, so the change shows up in the
    /// context menu without a sign-out.
    fn refresh_shell() {
        const SHCNE_ASSOCCHANGED: i32 = 0x0800_0000;
        const SHCNF_IDLIST: u32 = 0x0000;

        unsafe {
            SHChangeNotify(
                SHCNE_ASSOCCHANGED,
                SHCNF_IDLIST,
                std::ptr::null(),
                std::ptr::null(),
            );
        }
    }

    #[link(name = "shell32")]
    extern "system" {
        fn SHChangeNotify(
            event_id: i32,
            flags: u32,
            item1: *const std::ffi::c_void,
            item2: *const std::ffi::c_void,
        );
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use std::path::PathBuf;

    use anyhow::{anyhow, Context as _, Result};

    const DESKTOP_FILE: &str = "rustedpdf.desktop";

    fn applications_dir() -> Option<PathBuf> {
        let home = std::env::var_os("HOME")?;
        Some(
            PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("applications"),
        )
    }

    fn desktop_path() -> Option<PathBuf> {
        Some(applications_dir()?.join(DESKTOP_FILE))
    }

    pub fn is_registered() -> bool {
        desktop_path().is_some_and(|path| path.is_file())
    }

    /// The app icon, compiled in so a portable binary can install it without
    /// carrying loose files around.
    const ICON_PNG: &[u8] = include_bytes!("../assets/icon.png");
    const ICON_NAME: &str = "rustedpdf";

    fn icon_path() -> Option<PathBuf> {
        let home = std::env::var_os("HOME")?;
        Some(
            PathBuf::from(home)
                .join(".local/share/icons/hicolor/256x256/apps")
                .join(format!("{ICON_NAME}.png")),
        )
    }

    /// Writes the icon into the hicolor theme, where the desktop entry's
    /// `Icon=` name resolves it. Failure is not fatal: the entry still works,
    /// it just shows a generic icon.
    fn install_icon() {
        let Some(path) = icon_path() else { return };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(path, ICON_PNG);
    }

    pub fn register() -> Result<()> {
        let executable =
            std::env::current_exe().context("Could not find the RustedPdf executable")?;
        let directory = applications_dir().ok_or_else(|| anyhow!("No HOME directory"))?;
        std::fs::create_dir_all(&directory)?;
        install_icon();

        let entry = format!(
            "[Desktop Entry]\n\
             Type=Application\n\
             Name=RustedPdf\n\
             Comment=Read and highlight PDFs\n\
             Exec={} %f\n\
             Icon={ICON_NAME}\n\
             MimeType=application/pdf;\n\
             Terminal=false\n\
             Categories=Office;Viewer;\n",
            executable.display()
        );

        std::fs::write(directory.join(DESKTOP_FILE), entry)
            .context("Could not write the desktop entry")?;

        // Best effort: file managers pick the entry up on their own eventually.
        let _ = std::process::Command::new("update-desktop-database")
            .arg(&directory)
            .status();

        Ok(())
    }

    pub fn unregister() -> Result<()> {
        if let Some(path) = desktop_path() {
            let _ = std::fs::remove_file(path);
        }
        if let Some(path) = icon_path() {
            let _ = std::fs::remove_file(path);
        }
        if let Some(directory) = applications_dir() {
            let _ = std::process::Command::new("update-desktop-database")
                .arg(&directory)
                .status();
        }
        Ok(())
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use anyhow::{anyhow, Result};

    /// macOS reads document types from the .app bundle's Info.plist, which
    /// Launch Services indexes when the bundle is installed — there's nothing
    /// useful a running process can register at runtime.
    pub fn is_registered() -> bool {
        false
    }

    pub fn register() -> Result<()> {
        Err(anyhow!(
            "On macOS, \"Open with\" comes from the app bundle. Add a \
             CFBundleDocumentTypes entry for public.pdf to RustedPdf.app/Contents/Info.plist \
             and move the bundle to /Applications."
        ))
    }

    pub fn unregister() -> Result<()> {
        Ok(())
    }
}

/// Whether this build is currently registered for PDFs.
pub fn is_registered() -> bool {
    platform::is_registered()
}

/// Adds RustedPdf to the OS "Open with" menu for PDFs.
pub fn register() -> Result<()> {
    platform::register()
}

/// Removes what [`register`] added.
pub fn unregister() -> Result<()> {
    platform::unregister()
}
