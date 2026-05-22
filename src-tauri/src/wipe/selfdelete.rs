// Two-phase self-delete helper. The running app cannot remove its own
// executable on Windows, and even on Unix the bundled launchers
// (`.app` on macOS, `.AppImage` on Linux) have files that live alongside
// the running binary. The panic action wipes the data root synchronously
// and then stages a small platform-specific helper script to remove the
// app files on next boot/login.
//
// Contract 11 §Edge Cases: "If the self-delete helper cannot run
// (permissions, unusual install location) → the data wipe still fully
// succeeds; the user is told the app files may need manual removal."
//
// Failures from this module are always non-fatal and surface in the
// PanicWipeResult.self_delete_staged flag.

use std::io;
use std::path::PathBuf;

/// Stage the platform-appropriate self-delete helper. On Windows we drop
/// a small batch file into the user's Startup folder; on macOS/Linux we
/// drop a shell script into the user's autostart equivalent. The helper
/// removes the installed app files and then removes itself.
///
/// We do NOT trigger an immediate reboot — the contract requires that
/// "Later" be honored; the dialog asking for reboot is the IPC bridge's
/// responsibility (in the Settings UI), not this module's.
pub fn stage_self_delete() -> io::Result<()> {
    let exe = std::env::current_exe()?;
    let app_dir = exe
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "no parent dir for exe"))?
        .to_path_buf();

    #[cfg(target_os = "windows")]
    {
        stage_windows_helper(&exe, &app_dir)
    }
    #[cfg(target_os = "macos")]
    {
        stage_macos_helper(&exe, &app_dir)
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        stage_linux_helper(&exe, &app_dir)
    }
}

#[cfg(target_os = "windows")]
fn stage_windows_helper(exe: &std::path::Path, app_dir: &std::path::Path) -> io::Result<()> {
    let startup = startup_folder_windows()?;
    std::fs::create_dir_all(&startup)?;
    let helper = startup.join("cloudsaw-uninstall.cmd");

    // The script:
    //   1. Waits a few seconds to let the parent process exit cleanly.
    //   2. Tries to delete the app's install directory.
    //   3. Deletes itself.
    //
    // Quoting is critical — paths with spaces are common under Program
    // Files. We let cmd.exe's own parser handle quoted args; no shell
    // interpolation against user-controlled values happens here.
    let body = format!(
        "@echo off\r\n\
         timeout /t 5 /nobreak >nul\r\n\
         del /f /q \"{exe}\" 2>nul\r\n\
         rmdir /s /q \"{dir}\" 2>nul\r\n\
         del \"%~f0\" >nul 2>&1\r\n",
        exe = exe.display(),
        dir = app_dir.display(),
    );
    std::fs::write(&helper, body)?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn startup_folder_windows() -> io::Result<PathBuf> {
    if let Some(appdata) = std::env::var_os("APPDATA") {
        return Ok(PathBuf::from(appdata)
            .join("Microsoft")
            .join("Windows")
            .join("Start Menu")
            .join("Programs")
            .join("Startup"));
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "APPDATA not set; cannot stage self-delete helper",
    ))
}

#[cfg(target_os = "macos")]
fn stage_macos_helper(exe: &std::path::Path, app_dir: &std::path::Path) -> io::Result<()> {
    // macOS launchd LaunchAgent: drops a plist in ~/Library/LaunchAgents
    // that runs once at the next login.
    let home = std::env::var_os("HOME")
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HOME not set"))?;
    let agents = PathBuf::from(home).join("Library").join("LaunchAgents");
    std::fs::create_dir_all(&agents)?;
    let plist_path = agents.join("com.cloudsaw.uninstall.plist");
    let script_path = agents.join("cloudsaw-uninstall.sh");

    let script = format!(
        "#!/bin/sh\nsleep 5\nrm -rf \"{dir}\" 2>/dev/null\nrm -f \"{exe}\" 2>/dev/null\nlaunchctl unload \"{plist}\" 2>/dev/null\nrm -f \"{plist}\" \"$0\"\n",
        dir = app_dir.display(),
        exe = exe.display(),
        plist = plist_path.display(),
    );
    std::fs::write(&script_path, script)?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o700))?;

    let plist = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n\
         <dict>\n  <key>Label</key><string>com.cloudsaw.uninstall</string>\n  <key>ProgramArguments</key><array><string>/bin/sh</string><string>{script}</string></array>\n  <key>RunAtLoad</key><true/>\n</dict>\n</plist>\n",
        script = script_path.display(),
    );
    std::fs::write(&plist_path, plist)?;
    Ok(())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn stage_linux_helper(exe: &std::path::Path, app_dir: &std::path::Path) -> io::Result<()> {
    let home = std::env::var_os("HOME")
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HOME not set"))?;
    let autostart = PathBuf::from(home).join(".config").join("autostart");
    std::fs::create_dir_all(&autostart)?;
    let script = autostart.join("cloudsaw-uninstall.sh");
    let desktop = autostart.join("cloudsaw-uninstall.desktop");

    let body = format!(
        "#!/bin/sh\nsleep 5\nrm -rf \"{dir}\" 2>/dev/null\nrm -f \"{exe}\" 2>/dev/null\nrm -f \"{desktop}\" \"$0\"\n",
        dir = app_dir.display(),
        exe = exe.display(),
        desktop = desktop.display(),
    );
    std::fs::write(&script, body)?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700))?;

    let de = format!(
        "[Desktop Entry]\nType=Application\nName=CloudSaw Uninstall\nExec={script}\nHidden=false\nX-GNOME-Autostart-enabled=true\n",
        script = script.display(),
    );
    std::fs::write(&desktop, de)?;
    Ok(())
}

/// Reboot the machine at user-level. Best-effort and platform-specific.
/// The Settings UI offers Reboot/Later and only calls this on Reboot.
pub fn request_user_reboot() -> io::Result<()> {
    #[cfg(target_os = "windows")]
    {
        // `shutdown /r /t 0` schedules an immediate reboot. The /t 0
        // value is a parsed integer flag, not user-controlled text, so
        // no shell-interpolation concern applies.
        std::process::Command::new("shutdown")
            .args(["/r", "/t", "0"])
            .status()
            .map(|_| ())
    }
    #[cfg(target_os = "macos")]
    {
        // `osascript` user-level reboot prompt.
        std::process::Command::new("osascript")
            .args(["-e", "tell application \"System Events\" to restart"])
            .status()
            .map(|_| ())
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        // The graphical session bus respects this on most desktops; we
        // try logind first and fall back to `shutdown`.
        let logind = std::process::Command::new("systemctl")
            .args(["reboot"])
            .status();
        if logind.is_ok() {
            return Ok(());
        }
        std::process::Command::new("shutdown")
            .args(["-r", "now"])
            .status()
            .map(|_| ())
    }
}
