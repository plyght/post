use crate::{PostError, Result};
use std::path::Path;

/// XML escape utility function for plist generation
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Set secure file permissions on Unix systems
#[cfg(unix)]
fn set_file_permissions(path: &Path, mode: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let permissions = std::fs::Permissions::from_mode(mode);
    std::fs::set_permissions(path, permissions).map_err(PostError::Io)
}

/// Set secure file permissions on non-Unix systems (no-op)
#[cfg(not(unix))]
fn set_file_permissions(_path: &Path, _mode: u32) -> Result<()> {
    Ok(())
}

/// macOS-specific service management
#[cfg(target_os = "macos")]
pub mod macos {
    use super::*;

    /// Install the daemon as a macOS LaunchAgent service
    pub async fn install_service() -> Result<()> {
        let current_exe = std::env::current_exe().map_err(PostError::Io)?;
        let home_dir = dirs::home_dir()
            .ok_or_else(|| PostError::Other("Could not find home directory".to_string()))?;

        let plist_dir = home_dir.join("Library/LaunchAgents");
        std::fs::create_dir_all(&plist_dir).map_err(PostError::Io)?;

        // Set secure permissions on plist directory (755 - standard for LaunchAgents)
        set_file_permissions(&plist_dir, 0o755)?;

        let plist_path = plist_dir.join("com.post.daemon.plist");

        let current_exe_escaped = xml_escape(&current_exe.display().to_string());
        let log_path_escaped = xml_escape(&post_daemon::get_log_file_path()?.display().to_string());

        let plist_content = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.post.daemon</string>
    <key>ProgramArguments</key>
    <array>
        <string>{}</string>
        <string>daemon</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>{}</string>
    <key>StandardErrorPath</key>
    <string>{}</string>
</dict>
</plist>"#,
            current_exe_escaped, log_path_escaped, log_path_escaped
        );

        std::fs::write(&plist_path, plist_content).map_err(PostError::Io)?;

        // Set secure permissions on plist file (644 - readable by system)
        set_file_permissions(&plist_path, 0o644)?;

        // Load the service
        let output = tokio::process::Command::new("launchctl")
            .args([
                "load",
                plist_path
                    .to_str()
                    .ok_or_else(|| PostError::Other("Invalid plist path".to_string()))?,
            ])
            .output()
            .await
            .map_err(PostError::Io)?;

        if output.status.success() {
            println!("Service installed and started successfully!");
            println!("The daemon will start automatically on login.");
        } else {
            let error = String::from_utf8_lossy(&output.stderr);
            return Err(PostError::Other(format!(
                "Failed to load service: {}",
                error
            )));
        }

        Ok(())
    }

    /// Uninstall the macOS LaunchAgent service
    pub async fn uninstall_service() -> Result<()> {
        let home_dir = dirs::home_dir()
            .ok_or_else(|| PostError::Other("Could not find home directory".to_string()))?;

        let plist_path = home_dir.join("Library/LaunchAgents/com.post.daemon.plist");

        if plist_path.exists() {
            // Unload the service
            let output = tokio::process::Command::new("launchctl")
                .args([
                    "unload",
                    plist_path
                        .to_str()
                        .ok_or_else(|| PostError::Other("Invalid plist path".to_string()))?,
                ])
                .output()
                .await
                .map_err(PostError::Io)?;

            if !output.status.success() {
                let error = String::from_utf8_lossy(&output.stderr);
                eprintln!("Warning: Failed to unload service: {}", error);
            }

            // Remove the plist file
            std::fs::remove_file(&plist_path).map_err(PostError::Io)?;
            println!("Service uninstalled successfully!");
        } else {
            println!("Service is not installed.");
        }

        Ok(())
    }
}

/// Linux-specific service management
#[cfg(target_os = "linux")]
pub mod linux {
    use super::*;

    /// Install the daemon as a systemd user service
    pub async fn install_service() -> Result<()> {
        let current_exe = std::env::current_exe().map_err(PostError::Io)?;

        // Create systemd user service
        let home_dir = dirs::home_dir()
            .ok_or_else(|| PostError::Other("Could not find home directory".to_string()))?;

        let systemd_dir = home_dir.join(".config/systemd/user");
        std::fs::create_dir_all(&systemd_dir).map_err(PostError::Io)?;

        // Set secure permissions on systemd directory (755 - standard for systemd user services)
        set_file_permissions(&systemd_dir, 0o755)?;

        let service_path = systemd_dir.join("post-daemon.service");

        let service_content = format!(
            r#"[Unit]
Description=Post Clipboard Sync Daemon
After=network.target

[Service]
Type=simple
ExecStart={} daemon --foreground
Restart=always
RestartSec=5
StandardOutput=append:{}
StandardError=append:{}

[Install]
WantedBy=default.target
"#,
            current_exe.display(),
            post_daemon::get_log_file_path()?.display(),
            post_daemon::get_log_file_path()?.display()
        );

        std::fs::write(&service_path, service_content).map_err(PostError::Io)?;

        // Set secure permissions on service file (644 - readable by system)
        set_file_permissions(&service_path, 0o644)?;

        // Reload systemd and enable the service
        let reload_output = tokio::process::Command::new("systemctl")
            .args(&["--user", "daemon-reload"])
            .output()
            .await
            .map_err(PostError::Io)?;

        if !reload_output.status.success() {
            let error = String::from_utf8_lossy(&reload_output.stderr);
            return Err(PostError::Other(format!(
                "Failed to reload systemd: {}",
                error
            )));
        }

        let enable_output = tokio::process::Command::new("systemctl")
            .args(&["--user", "enable", "post-daemon.service"])
            .output()
            .await
            .map_err(PostError::Io)?;

        if !enable_output.status.success() {
            let error = String::from_utf8_lossy(&enable_output.stderr);
            return Err(PostError::Other(format!(
                "Failed to enable service: {}",
                error
            )));
        }

        let start_output = tokio::process::Command::new("systemctl")
            .args(&["--user", "start", "post-daemon.service"])
            .output()
            .await
            .map_err(PostError::Io)?;

        if start_output.status.success() {
            println!("Service installed, enabled, and started successfully!");
            println!("The daemon will start automatically on login.");
        } else {
            let error = String::from_utf8_lossy(&start_output.stderr);
            return Err(PostError::Other(format!(
                "Failed to start service: {}",
                error
            )));
        }

        Ok(())
    }

    /// Uninstall the systemd user service
    pub async fn uninstall_service() -> Result<()> {
        let home_dir = dirs::home_dir()
            .ok_or_else(|| PostError::Other("Could not find home directory".to_string()))?;

        let service_path = home_dir.join(".config/systemd/user/post-daemon.service");

        if service_path.exists() {
            // Stop and disable the service
            let _ = tokio::process::Command::new("systemctl")
                .args(&["--user", "stop", "post-daemon.service"])
                .output()
                .await;

            let _ = tokio::process::Command::new("systemctl")
                .args(&["--user", "disable", "post-daemon.service"])
                .output()
                .await;

            // Remove the service file
            std::fs::remove_file(&service_path).map_err(PostError::Io)?;

            // Reload systemd
            let _ = tokio::process::Command::new("systemctl")
                .args(&["--user", "daemon-reload"])
                .output()
                .await;

            println!("Service uninstalled successfully!");
        } else {
            println!("Service is not installed.");
        }

        Ok(())
    }
}

/// Cross-platform service management interface
pub async fn install_service() -> Result<()> {
    #[cfg(target_os = "macos")]
    return macos::install_service().await;

    #[cfg(target_os = "linux")]
    return linux::install_service().await;

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    return Err(PostError::Other(
        "Service installation is not supported on this platform".to_string(),
    ));
}

/// Cross-platform service uninstallation interface
pub async fn uninstall_service() -> Result<()> {
    #[cfg(target_os = "macos")]
    return macos::uninstall_service().await;

    #[cfg(target_os = "linux")]
    return linux::uninstall_service().await;

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    return Err(PostError::Other(
        "Service uninstallation is not supported on this platform".to_string(),
    ));
}
