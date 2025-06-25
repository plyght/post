use crate::{config::ClipboardConfig, PostError, Result};
use copypasta::{ClipboardContext, ClipboardProvider};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, warn};

#[async_trait::async_trait]
pub trait ClipboardManager: Send + Sync {
    async fn get_contents(&self) -> Result<String>;
    async fn set_contents(&self, content: &str) -> Result<()>;
}

#[async_trait::async_trait]
pub trait ClipboardWatcher: Send + Sync {
    async fn watch_changes(
        &self,
        callback: Box<dyn Fn(String) + Send + Sync + 'static>,
    ) -> Result<()>;
}

pub struct SystemClipboard {
    context: Arc<Mutex<ClipboardContext>>,
    last_content: Arc<Mutex<String>>,
}

impl SystemClipboard {
    pub fn new() -> Result<Self> {
        let context = ClipboardContext::new().map_err(|e| {
            PostError::Clipboard(format!("Failed to create clipboard context: {}", e))
        })?;

        Ok(Self {
            context: Arc::new(Mutex::new(context)),
            last_content: Arc::new(Mutex::new(String::new())),
        })
    }
}

/// Creates the best clipboard implementation for the current platform and environment
pub fn create_clipboard() -> Result<Box<dyn ClipboardManager>> {
    create_clipboard_with_config(&ClipboardConfig::default())
}

/// Creates the best clipboard watcher implementation for the current platform and environment
pub fn create_clipboard_watcher() -> Result<Box<dyn ClipboardWatcher>> {
    create_clipboard_watcher_with_config(&ClipboardConfig::default())
}

/// Creates clipboard implementation with specific configuration
pub fn create_clipboard_with_config(config: &ClipboardConfig) -> Result<Box<dyn ClipboardManager>> {
    #[cfg(target_os = "linux")]
    {
        match config.backend.as_str() {
            "wayland" => {
                if linux::has_wl_clipboard() {
                    debug!("Creating Wayland clipboard (forced via config)");
                    Ok(Box::new(linux::WaylandClipboard::new(
                        linux::WaylandClipboardType::Clipboard,
                    )?))
                } else {
                    return Err(PostError::Clipboard(
                        "Wayland clipboard requested but wl-clipboard not available".to_string(),
                    ));
                }
            }
            "xclip" => {
                if linux::has_xclip() {
                    debug!("Creating xclip clipboard (forced via config)");
                    Ok(Box::new(linux::XClipClipboard::new()?))
                } else {
                    return Err(PostError::Clipboard(
                        "xclip clipboard requested but xclip not available".to_string(),
                    ));
                }
            }
            "xsel" => {
                if linux::has_xsel() {
                    debug!("Creating xsel clipboard (forced via config)");
                    Ok(Box::new(linux::XSelClipboard::new()?))
                } else {
                    return Err(PostError::Clipboard(
                        "xsel clipboard requested but xsel not available".to_string(),
                    ));
                }
            }
            "system" => {
                debug!("Creating system clipboard (forced via config)");
                Ok(Box::new(SystemClipboard::new()?))
            }
            "auto" | _ => {
                if linux::is_wayland_session() && config.wayland_fallback {
                    debug!("Creating hybrid Linux clipboard for Wayland/Sway session");
                    Ok(Box::new(linux::HybridLinuxClipboard::new_with_config(
                        config,
                    )?))
                } else {
                    // Choose best X11 clipboard utility
                    let best_utility = linux::get_best_clipboard_utility();
                    match best_utility.as_str() {
                        "xclip" => {
                            debug!("Creating xclip clipboard for X11 session");
                            Ok(Box::new(linux::XClipClipboard::new()?))
                        }
                        "xsel" => {
                            debug!("Creating xsel clipboard for X11 session");
                            Ok(Box::new(linux::XSelClipboard::new()?))
                        }
                        _ => {
                            debug!("Creating system clipboard for X11 session");
                            Ok(Box::new(SystemClipboard::new()?))
                        }
                    }
                }
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        match config.backend.as_str() {
            "wsl" => {
                if windows::is_wsl_environment() {
                    debug!("Creating WSL clipboard (forced via config)");
                    Ok(Box::new(windows::WSLClipboard::new()?))
                } else {
                    return Err(PostError::Clipboard(
                        "WSL clipboard requested but not running in WSL environment".to_string(),
                    ));
                }
            }
            "windows" => {
                debug!("Creating Windows system clipboard (forced via config)");
                Ok(Box::new(SystemClipboard::new()?))
            }
            "auto" | _ => {
                if windows::is_wsl_environment() {
                    debug!("Creating WSL clipboard for WSL environment");
                    Ok(Box::new(windows::WSLClipboard::new()?))
                } else {
                    debug!("Creating Windows system clipboard");
                    Ok(Box::new(SystemClipboard::new()?))
                }
            }
        }
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        debug!(
            "Creating system clipboard for non-Linux/Windows platform (config: backend={})",
            config.backend
        );
        Ok(Box::new(SystemClipboard::new()?))
    }
}

/// Creates clipboard watcher implementation with specific configuration
pub fn create_clipboard_watcher_with_config(
    config: &ClipboardConfig,
) -> Result<Box<dyn ClipboardWatcher>> {
    #[cfg(target_os = "linux")]
    {
        match config.backend.as_str() {
            "wayland" => {
                return Err(PostError::Clipboard(
                    "Pure Wayland clipboard watching not supported, use hybrid mode".to_string(),
                ));
            }
            "xclip" => {
                if linux::has_xclip() {
                    debug!("Creating xclip clipboard watcher (forced via config)");
                    Ok(Box::new(linux::XClipClipboard::new()?))
                } else {
                    return Err(PostError::Clipboard(
                        "xclip clipboard watcher requested but xclip not available".to_string(),
                    ));
                }
            }
            "xsel" => {
                if linux::has_xsel() {
                    debug!("Creating xsel clipboard watcher (forced via config)");
                    Ok(Box::new(linux::XSelClipboard::new()?))
                } else {
                    return Err(PostError::Clipboard(
                        "xsel clipboard watcher requested but xsel not available".to_string(),
                    ));
                }
            }
            "system" => {
                debug!("Creating system clipboard watcher (forced via config)");
                Ok(Box::new(SystemClipboard::new()?))
            }
            "auto" | _ => {
                if linux::is_wayland_session() && config.wayland_fallback {
                    debug!("Creating hybrid Linux clipboard watcher for Wayland/Sway session");
                    Ok(Box::new(linux::HybridLinuxClipboard::new_with_config(
                        config,
                    )?))
                } else {
                    // Choose best X11 clipboard utility
                    let best_utility = linux::get_best_clipboard_utility();
                    match best_utility.as_str() {
                        "xclip" => {
                            debug!("Creating xclip clipboard watcher for X11 session");
                            Ok(Box::new(linux::XClipClipboard::new()?))
                        }
                        "xsel" => {
                            debug!("Creating xsel clipboard watcher for X11 session");
                            Ok(Box::new(linux::XSelClipboard::new()?))
                        }
                        _ => {
                            debug!("Creating system clipboard watcher for X11 session");
                            Ok(Box::new(SystemClipboard::new()?))
                        }
                    }
                }
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        match config.backend.as_str() {
            "wsl" => {
                if windows::is_wsl_environment() {
                    debug!("Creating WSL clipboard watcher (forced via config)");
                    Ok(Box::new(windows::WSLClipboard::new()?))
                } else {
                    return Err(PostError::Clipboard(
                        "WSL clipboard watcher requested but not running in WSL environment".to_string(),
                    ));
                }
            }
            "windows" => {
                debug!("Creating Windows system clipboard watcher (forced via config)");
                Ok(Box::new(SystemClipboard::new()?))
            }
            "auto" | _ => {
                if windows::is_wsl_environment() {
                    debug!("Creating WSL clipboard watcher for WSL environment");
                    Ok(Box::new(windows::WSLClipboard::new()?))
                } else {
                    debug!("Creating Windows system clipboard watcher");
                    Ok(Box::new(SystemClipboard::new()?))
                }
            }
        }
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        debug!(
            "Creating system clipboard watcher for non-Linux/Windows platform (config: backend={})",
            config.backend
        );
        Ok(Box::new(SystemClipboard::new()?))
    }
}

#[async_trait::async_trait]
impl ClipboardManager for SystemClipboard {
    async fn get_contents(&self) -> Result<String> {
        let mut ctx = self.context.lock().await;
        ctx.get_contents()
            .map_err(|e| PostError::Clipboard(format!("Failed to get clipboard contents: {}", e)))
    }

    async fn set_contents(&self, content: &str) -> Result<()> {
        let mut ctx = self.context.lock().await;
        ctx.set_contents(content.to_owned()).map_err(|e| {
            PostError::Clipboard(format!("Failed to set clipboard contents: {}", e))
        })?;

        let mut last = self.last_content.lock().await;
        *last = content.to_owned();

        debug!("Set clipboard contents: {} chars", content.len());
        Ok(())
    }
}

#[async_trait::async_trait]
impl ClipboardWatcher for SystemClipboard {
    async fn watch_changes(
        &self,
        callback: Box<dyn Fn(String) + Send + Sync + 'static>,
    ) -> Result<()> {
        let clipboard = Arc::clone(&self.context);
        let last_content = Arc::clone(&self.last_content);

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_millis(500));

            loop {
                interval.tick().await;

                let current_content = {
                    let mut ctx = clipboard.lock().await;
                    match ctx.get_contents() {
                        Ok(content) => content,
                        Err(e) => {
                            warn!("Failed to check clipboard: {}", e);
                            continue;
                        }
                    }
                };

                let mut last = last_content.lock().await;
                if current_content != *last && !current_content.is_empty() {
                    *last = current_content.clone();
                    drop(last);

                    debug!("Clipboard changed: {} chars", current_content.len());
                    callback(current_content);
                }
            }
        });

        Ok(())
    }
}

impl SystemClipboard {
    pub async fn watch_changes_generic<F>(&self, callback: F) -> Result<()>
    where
        F: Fn(String) + Send + Sync + 'static,
    {
        let clipboard = Arc::clone(&self.context);
        let last_content = Arc::clone(&self.last_content);

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_millis(500));

            loop {
                interval.tick().await;

                let current_content = {
                    let mut ctx = clipboard.lock().await;
                    match ctx.get_contents() {
                        Ok(content) => content,
                        Err(e) => {
                            warn!("Failed to check clipboard: {}", e);
                            continue;
                        }
                    }
                };

                let mut last = last_content.lock().await;
                if current_content != *last && !current_content.is_empty() {
                    *last = current_content.clone();
                    drop(last);

                    debug!("Clipboard changed: {} chars", current_content.len());
                    callback(current_content);
                }
            }
        });

        Ok(())
    }
}

#[cfg(target_os = "linux")]
pub mod linux {
    use super::*;
    use std::env;
    use std::process::Command;
    use tokio::process::Command as TokioCommand;

    #[derive(Debug, Clone)]
    pub enum WaylandClipboardType {
        Primary,
        Clipboard,
    }

    pub fn is_wayland_session() -> bool {
        env::var("WAYLAND_DISPLAY").is_ok()
            || env::var("XDG_SESSION_TYPE")
                .map(|s| s == "wayland")
                .unwrap_or(false)
    }

    pub fn is_sway_session() -> bool {
        is_wayland_session()
            && (env::var("SWAYSOCK").is_ok()
                || env::var("XDG_CURRENT_DESKTOP")
                    .map(|s| s.contains("sway"))
                    .unwrap_or(false)
                || env::var("DESKTOP_SESSION")
                    .map(|s| s.contains("sway"))
                    .unwrap_or(false))
    }

    pub fn has_wl_clipboard() -> bool {
        Command::new("which")
            .arg("wl-copy")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
            && Command::new("which")
                .arg("wl-paste")
                .output()
                .map(|output| output.status.success())
                .unwrap_or(false)
    }

    pub fn has_xclip() -> bool {
        Command::new("which")
            .arg("xclip")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    pub fn has_xsel() -> bool {
        Command::new("which")
            .arg("xsel")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    pub fn detect_desktop_environment() -> String {
        // Check for specific desktop environments
        if let Ok(desktop) = env::var("XDG_CURRENT_DESKTOP") {
            return desktop.to_lowercase();
        }

        if let Ok(session) = env::var("DESKTOP_SESSION") {
            return session.to_lowercase();
        }

        if let Ok(gdm_session) = env::var("GDMSESSION") {
            return gdm_session.to_lowercase();
        }

        // Check for specific window managers
        if env::var("KDE_FULL_SESSION").is_ok() || env::var("KDE_SESSION_VERSION").is_ok() {
            return "kde".to_string();
        }

        if env::var("GNOME_DESKTOP_SESSION_ID").is_ok() {
            return "gnome".to_string();
        }

        if env::var("I3SOCK").is_ok() {
            return "i3".to_string();
        }

        // Check for running window managers via pgrep
        let wm_checks = [
            ("i3", "i3"),
            ("dwm", "dwm"),
            ("awesome", "awesome"),
            ("bspwm", "bspwm"),
            ("qtile", "qtile"),
            ("xmonad", "xmonad"),
            ("openbox", "openbox"),
            ("fluxbox", "fluxbox"),
        ];

        for (wm_name, process_name) in &wm_checks {
            if Command::new("pgrep")
                .arg(process_name)
                .output()
                .map(|output| output.status.success())
                .unwrap_or(false)
            {
                return wm_name.to_string();
            }
        }

        "unknown".to_string()
    }

    pub fn get_best_clipboard_utility() -> String {
        if is_wayland_session() && has_wl_clipboard() {
            return "wl-clipboard".to_string();
        }

        if has_xclip() {
            return "xclip".to_string();
        }

        if has_xsel() {
            return "xsel".to_string();
        }

        "system".to_string()
    }

    /// Provides detailed diagnostics for clipboard issues in Linux environments
    pub fn diagnose_clipboard_environment() -> Result<String> {
        let mut diagnostics = Vec::new();

        // Environment detection
        diagnostics.push(format!("Wayland session: {}", is_wayland_session()));
        diagnostics.push(format!("Sway session: {}", is_sway_session()));
        diagnostics.push(format!("Desktop environment: {}", detect_desktop_environment()));
        diagnostics.push(format!("Best clipboard utility: {}", get_best_clipboard_utility()));

        // Environment variables
        diagnostics.push(format!(
            "WAYLAND_DISPLAY: {:?}",
            env::var("WAYLAND_DISPLAY")
        ));
        diagnostics.push(format!(
            "XDG_SESSION_TYPE: {:?}",
            env::var("XDG_SESSION_TYPE")
        ));
        diagnostics.push(format!(
            "XDG_CURRENT_DESKTOP: {:?}",
            env::var("XDG_CURRENT_DESKTOP")
        ));
        diagnostics.push(format!("SWAYSOCK: {:?}", env::var("SWAYSOCK")));
        diagnostics.push(format!(
            "DESKTOP_SESSION: {:?}",
            env::var("DESKTOP_SESSION")
        ));
        diagnostics.push(format!("DISPLAY: {:?}", env::var("DISPLAY")));

        // Tool availability
        diagnostics.push(format!("wl-clipboard available: {}", has_wl_clipboard()));
        diagnostics.push(format!("xclip available: {}", has_xclip()));
        diagnostics.push(format!("xsel available: {}", has_xsel()));

        // Test clipboard access if tools are available
        if has_wl_clipboard() {
            let clipboard_test = std::process::Command::new("wl-paste")
                .arg("--clipboard")
                .arg("--no-newline")
                .output();

            match clipboard_test {
                Ok(output) => {
                    if output.status.success() {
                        diagnostics.push("wl-clipboard access: OK".to_string());
                    } else {
                        let stderr = String::from_utf8_lossy(&output.stderr);
                        diagnostics.push(format!("wl-clipboard access: FAILED - {}", stderr));
                    }
                }
                Err(e) => {
                    diagnostics.push(format!("wl-clipboard access: ERROR - {}", e));
                }
            }
        }

        if has_xclip() {
            let clipboard_test = std::process::Command::new("xclip")
                .arg("-selection")
                .arg("clipboard")
                .arg("-o")
                .output();

            match clipboard_test {
                Ok(output) => {
                    if output.status.success() {
                        diagnostics.push("xclip access: OK".to_string());
                    } else {
                        let stderr = String::from_utf8_lossy(&output.stderr);
                        diagnostics.push(format!("xclip access: FAILED - {}", stderr));
                    }
                }
                Err(e) => {
                    diagnostics.push(format!("xclip access: ERROR - {}", e));
                }
            }
        }

        if has_xsel() {
            let clipboard_test = std::process::Command::new("xsel")
                .arg("--clipboard")
                .arg("--output")
                .output();

            match clipboard_test {
                Ok(output) => {
                    if output.status.success() {
                        diagnostics.push("xsel access: OK".to_string());
                    } else {
                        let stderr = String::from_utf8_lossy(&output.stderr);
                        diagnostics.push(format!("xsel access: FAILED - {}", stderr));
                    }
                }
                Err(e) => {
                    diagnostics.push(format!("xsel access: ERROR - {}", e));
                }
            }
        }

        // Recommendations
        if is_wayland_session() && !has_wl_clipboard() {
            diagnostics.push("RECOMMENDATION: Install wl-clipboard package for optimal Wayland clipboard support".to_string());
        }

        if !is_wayland_session() && !has_xclip() && !has_xsel() {
            diagnostics.push("RECOMMENDATION: Install xclip or xsel package for better X11 clipboard support".to_string());
        }

        if is_sway_session() {
            diagnostics.push("INFO: Sway-specific optimizations are available".to_string());
        }

        Ok(diagnostics.join("\n"))
    }

    /// Creates an enhanced error with environment context for troubleshooting
    pub fn create_contextual_error(base_error: &str) -> PostError {
        let context = match diagnose_clipboard_environment() {
            Ok(diag) => format!("{}\n\nEnvironment Diagnostics:\n{}", base_error, diag),
            Err(_) => base_error.to_string(),
        };

        PostError::Clipboard(context)
    }

    pub struct WaylandClipboard {
        clipboard_type: WaylandClipboardType,
    }

    impl WaylandClipboard {
        pub fn new(clipboard_type: WaylandClipboardType) -> Result<Self> {
            if !has_wl_clipboard() {
                return Err(create_contextual_error(
                    "wl-clipboard utilities (wl-copy/wl-paste) not found",
                ));
            }
            Ok(Self { clipboard_type })
        }

        async fn get_clipboard_contents(&self) -> Result<String> {
            let selection_arg = match self.clipboard_type {
                WaylandClipboardType::Primary => "--primary",
                WaylandClipboardType::Clipboard => "--clipboard",
            };

            let output = TokioCommand::new("wl-paste")
                .arg(selection_arg)
                .arg("--no-newline")
                .output()
                .await
                .map_err(|e| PostError::Clipboard(format!("Failed to execute wl-paste: {}", e)))?;

            if !output.status.success() {
                // Empty clipboard is not an error - wl-paste exits with code 1 when clipboard is empty
                if output.status.code() == Some(1) {
                    return Ok(String::new());
                }
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(PostError::Clipboard(format!("wl-paste failed: {}", stderr)));
            }

            String::from_utf8(output.stdout)
                .map_err(|e| PostError::Clipboard(format!("Invalid UTF-8 in clipboard: {}", e)))
        }

        async fn set_clipboard_contents(&self, content: &str) -> Result<()> {
            let selection_arg = match self.clipboard_type {
                WaylandClipboardType::Primary => "--primary",
                WaylandClipboardType::Clipboard => "--clipboard",
            };

            let mut cmd = TokioCommand::new("wl-copy")
                .arg(selection_arg)
                .arg("--type")
                .arg("text/plain")
                .stdin(std::process::Stdio::piped())
                .spawn()
                .map_err(|e| PostError::Clipboard(format!("Failed to execute wl-copy: {}", e)))?;

            if let Some(stdin) = cmd.stdin.as_mut() {
                use tokio::io::AsyncWriteExt;
                stdin.write_all(content.as_bytes()).await.map_err(|e| {
                    PostError::Clipboard(format!("Failed to write to wl-copy: {}", e))
                })?;
                stdin.shutdown().await.map_err(|e| {
                    PostError::Clipboard(format!("Failed to close wl-copy stdin: {}", e))
                })?;
            }

            let status = cmd
                .wait()
                .await
                .map_err(|e| PostError::Clipboard(format!("Failed to wait for wl-copy: {}", e)))?;

            if !status.success() {
                return Err(PostError::Clipboard(format!(
                    "wl-copy failed with exit code: {:?}",
                    status.code()
                )));
            }

            debug!(
                "Set Wayland clipboard contents via wl-copy: {} chars",
                content.len()
            );
            Ok(())
        }
    }

    #[async_trait::async_trait]
    impl ClipboardManager for WaylandClipboard {
        async fn get_contents(&self) -> Result<String> {
            self.get_clipboard_contents().await
        }

        async fn set_contents(&self, content: &str) -> Result<()> {
            self.set_clipboard_contents(content).await
        }
    }

    pub struct XClipClipboard {
        last_content: Arc<Mutex<String>>,
    }

    impl XClipClipboard {
        pub fn new() -> Result<Self> {
            if !has_xclip() {
                return Err(create_contextual_error(
                    "xclip utility not found",
                ));
            }
            Ok(Self {
                last_content: Arc::new(Mutex::new(String::new())),
            })
        }

        async fn get_clipboard_contents(&self) -> Result<String> {
            let output = TokioCommand::new("xclip")
                .arg("-selection")
                .arg("clipboard")
                .arg("-o")
                .output()
                .await
                .map_err(|e| PostError::Clipboard(format!("Failed to execute xclip: {}", e)))?;

            if !output.status.success() {
                // Empty clipboard is not an error - xclip exits with code 1 when clipboard is empty
                if output.status.code() == Some(1) {
                    return Ok(String::new());
                }
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(PostError::Clipboard(format!("xclip failed: {}", stderr)));
            }

            String::from_utf8(output.stdout)
                .map_err(|e| PostError::Clipboard(format!("Invalid UTF-8 in clipboard: {}", e)))
        }

        async fn set_clipboard_contents(&self, content: &str) -> Result<()> {
            let mut cmd = TokioCommand::new("xclip")
                .arg("-selection")
                .arg("clipboard")
                .arg("-i")
                .stdin(std::process::Stdio::piped())
                .spawn()
                .map_err(|e| PostError::Clipboard(format!("Failed to execute xclip: {}", e)))?;

            if let Some(stdin) = cmd.stdin.as_mut() {
                use tokio::io::AsyncWriteExt;
                stdin.write_all(content.as_bytes()).await.map_err(|e| {
                    PostError::Clipboard(format!("Failed to write to xclip: {}", e))
                })?;
                stdin.shutdown().await.map_err(|e| {
                    PostError::Clipboard(format!("Failed to close xclip stdin: {}", e))
                })?;
            }

            let status = cmd
                .wait()
                .await
                .map_err(|e| PostError::Clipboard(format!("Failed to wait for xclip: {}", e)))?;

            if !status.success() {
                return Err(PostError::Clipboard(format!(
                    "xclip failed with exit code: {:?}",
                    status.code()
                )));
            }

            debug!("Set X11 clipboard contents via xclip: {} chars", content.len());
            Ok(())
        }
    }

    #[async_trait::async_trait]
    impl ClipboardManager for XClipClipboard {
        async fn get_contents(&self) -> Result<String> {
            self.get_clipboard_contents().await
        }

        async fn set_contents(&self, content: &str) -> Result<()> {
            self.set_clipboard_contents(content).await?;

            let mut last = self.last_content.lock().await;
            *last = content.to_owned();

            Ok(())
        }
    }

    #[async_trait::async_trait]
    impl ClipboardWatcher for XClipClipboard {
        async fn watch_changes(
            &self,
            callback: Box<dyn Fn(String) + Send + Sync + 'static>,
        ) -> Result<()> {
            let last_content = Arc::clone(&self.last_content);

            tokio::spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_millis(500));

                loop {
                    interval.tick().await;

                    let current_content = {
                        let output = TokioCommand::new("xclip")
                            .arg("-selection")
                            .arg("clipboard")
                            .arg("-o")
                            .output()
                            .await;

                        match output {
                            Ok(output) if output.status.success() => {
                                String::from_utf8_lossy(&output.stdout).to_string()
                            }
                            _ => {
                                warn!("Failed to check clipboard via xclip");
                                continue;
                            }
                        }
                    };

                    let mut last = last_content.lock().await;
                    if current_content != *last && !current_content.is_empty() {
                        *last = current_content.clone();
                        drop(last);

                        debug!("X11 clipboard changed via xclip: {} chars", current_content.len());
                        callback(current_content);
                    }
                }
            });

            Ok(())
        }
    }

    pub struct XSelClipboard {
        last_content: Arc<Mutex<String>>,
    }

    impl XSelClipboard {
        pub fn new() -> Result<Self> {
            if !has_xsel() {
                return Err(create_contextual_error(
                    "xsel utility not found",
                ));
            }
            Ok(Self {
                last_content: Arc::new(Mutex::new(String::new())),
            })
        }

        async fn get_clipboard_contents(&self) -> Result<String> {
            let output = TokioCommand::new("xsel")
                .arg("--clipboard")
                .arg("--output")
                .output()
                .await
                .map_err(|e| PostError::Clipboard(format!("Failed to execute xsel: {}", e)))?;

            if !output.status.success() {
                // Empty clipboard is not an error - xsel exits with code 1 when clipboard is empty
                if output.status.code() == Some(1) {
                    return Ok(String::new());
                }
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(PostError::Clipboard(format!("xsel failed: {}", stderr)));
            }

            String::from_utf8(output.stdout)
                .map_err(|e| PostError::Clipboard(format!("Invalid UTF-8 in clipboard: {}", e)))
        }

        async fn set_clipboard_contents(&self, content: &str) -> Result<()> {
            let mut cmd = TokioCommand::new("xsel")
                .arg("--clipboard")
                .arg("--input")
                .stdin(std::process::Stdio::piped())
                .spawn()
                .map_err(|e| PostError::Clipboard(format!("Failed to execute xsel: {}", e)))?;

            if let Some(stdin) = cmd.stdin.as_mut() {
                use tokio::io::AsyncWriteExt;
                stdin.write_all(content.as_bytes()).await.map_err(|e| {
                    PostError::Clipboard(format!("Failed to write to xsel: {}", e))
                })?;
                stdin.shutdown().await.map_err(|e| {
                    PostError::Clipboard(format!("Failed to close xsel stdin: {}", e))
                })?;
            }

            let status = cmd
                .wait()
                .await
                .map_err(|e| PostError::Clipboard(format!("Failed to wait for xsel: {}", e)))?;

            if !status.success() {
                return Err(PostError::Clipboard(format!(
                    "xsel failed with exit code: {:?}",
                    status.code()
                )));
            }

            debug!("Set X11 clipboard contents via xsel: {} chars", content.len());
            Ok(())
        }
    }

    #[async_trait::async_trait]
    impl ClipboardManager for XSelClipboard {
        async fn get_contents(&self) -> Result<String> {
            self.get_clipboard_contents().await
        }

        async fn set_contents(&self, content: &str) -> Result<()> {
            self.set_clipboard_contents(content).await?;

            let mut last = self.last_content.lock().await;
            *last = content.to_owned();

            Ok(())
        }
    }

    #[async_trait::async_trait]
    impl ClipboardWatcher for XSelClipboard {
        async fn watch_changes(
            &self,
            callback: Box<dyn Fn(String) + Send + Sync + 'static>,
        ) -> Result<()> {
            let last_content = Arc::clone(&self.last_content);

            tokio::spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_millis(500));

                loop {
                    interval.tick().await;

                    let current_content = {
                        let output = TokioCommand::new("xsel")
                            .arg("--clipboard")
                            .arg("--output")
                            .output()
                            .await;

                        match output {
                            Ok(output) if output.status.success() => {
                                String::from_utf8_lossy(&output.stdout).to_string()
                            }
                            _ => {
                                warn!("Failed to check clipboard via xsel");
                                continue;
                            }
                        }
                    };

                    let mut last = last_content.lock().await;
                    if current_content != *last && !current_content.is_empty() {
                        *last = current_content.clone();
                        drop(last);

                        debug!("X11 clipboard changed via xsel: {} chars", current_content.len());
                        callback(current_content);
                    }
                }
            });

            Ok(())
        }
    }

    pub struct HybridLinuxClipboard {
        wayland_clipboard: Option<WaylandClipboard>,
        system_clipboard: SystemClipboard,
        last_content: Arc<Mutex<String>>,
        config: ClipboardConfig,
    }

    impl HybridLinuxClipboard {
        pub fn new() -> Result<Self> {
            Self::new_with_config(&ClipboardConfig::default())
        }

        pub fn new_with_config(config: &ClipboardConfig) -> Result<Self> {
            let system_clipboard = SystemClipboard::new()?;

            let wayland_clipboard = if is_wayland_session()
                && has_wl_clipboard()
                && config.wayland_fallback
            {
                debug!(
                    "Detected Wayland session with wl-clipboard, enabling hybrid clipboard support"
                );

                if config.sway_optimizations && is_sway_session() {
                    debug!("Enabling Sway-specific clipboard optimizations");
                }

                Some(WaylandClipboard::new(WaylandClipboardType::Clipboard)?)
            } else {
                None
            };

            Ok(Self {
                wayland_clipboard,
                system_clipboard,
                last_content: Arc::new(Mutex::new(String::new())),
                config: config.clone(),
            })
        }

        async fn get_preferred_contents(&self) -> Result<String> {
            let content = if let Some(ref wayland_clipboard) = self.wayland_clipboard {
                match wayland_clipboard.get_contents().await {
                    Ok(content) => content,
                    Err(e) => {
                        debug!(
                            "Wayland clipboard get failed, falling back to system clipboard: {}",
                            e
                        );
                        self.system_clipboard.get_contents().await?
                    }
                }
            } else {
                self.system_clipboard.get_contents().await?
            };

            // Check content size limit
            if content.len() > self.config.max_content_size {
                debug!(
                    "Clipboard content too large ({} bytes), truncating to {} bytes",
                    content.len(),
                    self.config.max_content_size
                );
                Ok(content.chars().take(self.config.max_content_size).collect())
            } else {
                Ok(content)
            }
        }

        async fn set_preferred_contents(&self, content: &str) -> Result<()> {
            // Check content size limit before setting
            if content.len() > self.config.max_content_size {
                return Err(PostError::Clipboard(format!(
                    "Content too large ({} bytes), maximum allowed: {} bytes",
                    content.len(),
                    self.config.max_content_size
                )));
            }

            // Always try system clipboard first for compatibility
            let system_result = self.system_clipboard.set_contents(content).await;

            if let Some(ref wayland_clipboard) = self.wayland_clipboard {
                match wayland_clipboard.set_contents(content).await {
                    Ok(_) => {
                        debug!("Set content in both system and Wayland clipboards");
                        return system_result;
                    }
                    Err(e) => {
                        debug!(
                            "Wayland clipboard set failed, using system clipboard only: {}",
                            e
                        );
                    }
                }
            }

            system_result
        }
    }

    #[async_trait::async_trait]
    impl ClipboardManager for HybridLinuxClipboard {
        async fn get_contents(&self) -> Result<String> {
            self.get_preferred_contents().await
        }

        async fn set_contents(&self, content: &str) -> Result<()> {
            self.set_preferred_contents(content).await?;

            let mut last = self.last_content.lock().await;
            *last = content.to_owned();

            Ok(())
        }
    }

    #[async_trait::async_trait]
    impl ClipboardWatcher for HybridLinuxClipboard {
        async fn watch_changes(
            &self,
            callback: Box<dyn Fn(String) + Send + Sync + 'static>,
        ) -> Result<()> {
            let wayland_clipboard = self.wayland_clipboard.clone();
            let system_clipboard = Arc::clone(&self.system_clipboard.context);
            let last_content = Arc::clone(&self.last_content);

            let poll_interval = self.config.poll_interval_ms;
            tokio::spawn(async move {
                let mut interval =
                    tokio::time::interval(std::time::Duration::from_millis(poll_interval));

                loop {
                    interval.tick().await;

                    // Try Wayland clipboard first if available
                    let current_content = if let Some(ref wayland_cb) = wayland_clipboard {
                        match wayland_cb.get_contents().await {
                            Ok(content) => content,
                            Err(_) => {
                                // Fall back to system clipboard
                                let mut ctx = system_clipboard.lock().await;
                                match ctx.get_contents() {
                                    Ok(content) => content,
                                    Err(e) => {
                                        warn!("Failed to check both clipboards: {}", e);
                                        continue;
                                    }
                                }
                            }
                        }
                    } else {
                        // Use system clipboard only
                        let mut ctx = system_clipboard.lock().await;
                        match ctx.get_contents() {
                            Ok(content) => content,
                            Err(e) => {
                                warn!("Failed to check clipboard: {}", e);
                                continue;
                            }
                        }
                    };

                    let mut last = last_content.lock().await;
                    if current_content != *last && !current_content.is_empty() {
                        *last = current_content.clone();
                        drop(last);

                        debug!("Clipboard changed: {} chars", current_content.len());
                        callback(current_content);
                    }
                }
            });

            Ok(())
        }
    }
}

#[cfg(target_os = "macos")]
pub mod macos {
    use super::*;
    use std::os::raw::c_void;

    extern "C" {
        fn NSPasteboardNameGeneral() -> *const c_void;
        fn objc_msgSend(receiver: *const c_void, selector: *const c_void, ...) -> *const c_void;
        fn sel_registerName(name: *const i8) -> *const c_void;
    }

    static UNIVERSAL_CLIPBOARD_SUPPRESSED: AtomicBool = AtomicBool::new(false);

    pub fn suppress_universal_clipboard() -> Result<()> {
        UNIVERSAL_CLIPBOARD_SUPPRESSED.store(true, Ordering::Relaxed);
        debug!("Suppressing macOS Universal Clipboard for this session");
        Ok(())
    }

    pub fn detect_universal_clipboard_event() -> Result<bool> {
        if !UNIVERSAL_CLIPBOARD_SUPPRESSED.load(Ordering::Relaxed) {
            unsafe {
                // Basic detection - check if pasteboard name is accessible
                let pasteboard_name = NSPasteboardNameGeneral();
                if pasteboard_name.is_null() {
                    debug!("Universal Clipboard event detected");
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    pub fn is_universal_clipboard_available() -> Result<bool> {
        unsafe {
            let pasteboard_name = NSPasteboardNameGeneral();
            Ok(!pasteboard_name.is_null())
        }
    }

    pub fn get_pasteboard_change_count() -> Result<i64> {
        unsafe {
            let pasteboard_name = NSPasteboardNameGeneral();
            if pasteboard_name.is_null() {
                return Ok(0);
            }

            let change_count_sel = sel_registerName(c"changeCount".as_ptr());
            let change_count = objc_msgSend(pasteboard_name, change_count_sel) as i64;

            Ok(change_count)
        }
    }
}

#[cfg(target_os = "windows")]
pub mod windows {
    use super::*;
    use std::env;
    use std::fs;
    use tokio::process::Command as TokioCommand;
    use std::process::Command;

    pub fn is_wsl_environment() -> bool {
        // Check for WSL-specific environment variables
        if env::var("WSL_DISTRO_NAME").is_ok() 
            || env::var("WSL_INTEROP").is_ok() 
            || env::var("WSLENV").is_ok() {
            return true;
        }

        // Check for WSL in /proc/version (fallback for WSL1)
        if let Ok(version) = fs::read_to_string("/proc/version") {
            if version.to_lowercase().contains("microsoft") || version.to_lowercase().contains("wsl") {
                return true;
            }
        }

        // Check for Windows paths in PATH (additional fallback)
        if let Ok(path) = env::var("PATH") {
            if path.contains("/mnt/c/") || path.contains(":/mnt/c") {
                return true;
            }
        }

        false
    }

    pub fn is_clip_exe_available() -> bool {
        // Check if Windows clip.exe is available (for WSL)
        Command::new("clip.exe")
            .arg("/?")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    pub fn is_powershell_available() -> bool {
        // Check if PowerShell is available for advanced clipboard operations
        Command::new("powershell.exe")
            .arg("-Command")
            .arg("Get-Clipboard")
            .arg("-Format")
            .arg("Text")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    /// Provides detailed diagnostics for clipboard issues in Windows/WSL environments
    pub fn diagnose_clipboard_environment() -> Result<String> {
        let mut diagnostics = Vec::new();

        // Environment detection
        diagnostics.push(format!("WSL environment: {}", is_wsl_environment()));
        diagnostics.push(format!("clip.exe available: {}", is_clip_exe_available()));
        diagnostics.push(format!("PowerShell available: {}", is_powershell_available()));

        // Environment variables
        diagnostics.push(format!("WSL_DISTRO_NAME: {:?}", env::var("WSL_DISTRO_NAME")));
        diagnostics.push(format!("WSL_INTEROP: {:?}", env::var("WSL_INTEROP")));
        diagnostics.push(format!("WSLENV: {:?}", env::var("WSLENV")));

        // Windows paths in PATH
        if let Ok(path) = env::var("PATH") {
            let has_windows_paths = path.contains("/mnt/c/") || path.contains(":/mnt/c");
            diagnostics.push(format!("Windows paths in PATH: {}", has_windows_paths));
        }

        // Test clipboard access
        if is_wsl_environment() && is_clip_exe_available() {
            let clipboard_test = std::process::Command::new("powershell.exe")
                .arg("-Command")
                .arg("Get-Clipboard")
                .arg("-Format")
                .arg("Text")
                .output();

            match clipboard_test {
                Ok(output) => {
                    if output.status.success() {
                        diagnostics.push("Clipboard access: OK".to_string());
                    } else {
                        let stderr = String::from_utf8_lossy(&output.stderr);
                        diagnostics.push(format!("Clipboard access: FAILED - {}", stderr));
                    }
                }
                Err(e) => {
                    diagnostics.push(format!("Clipboard access: ERROR - {}", e));
                }
            }
        }

        // Recommendations
        if is_wsl_environment() && !is_clip_exe_available() {
            diagnostics.push("RECOMMENDATION: Ensure Windows clipboard utilities are available from WSL".to_string());
        }

        Ok(diagnostics.join("\n"))
    }

    /// Creates an enhanced error with environment context for troubleshooting
    pub fn create_contextual_error(base_error: &str) -> PostError {
        let context = match diagnose_clipboard_environment() {
            Ok(diag) => format!("{}\n\nEnvironment Diagnostics:\n{}", base_error, diag),
            Err(_) => base_error.to_string(),
        };

        PostError::Clipboard(context)
    }

    pub struct WSLClipboard {
        last_content: Arc<Mutex<String>>,
    }

    impl WSLClipboard {
        pub fn new() -> Result<Self> {
            if !is_wsl_environment() {
                return Err(create_contextual_error(
                    "WSL clipboard requested but not running in WSL environment",
                ));
            }

            if !is_clip_exe_available() && !is_powershell_available() {
                return Err(create_contextual_error(
                    "Neither clip.exe nor PowerShell are available for WSL clipboard operations",
                ));
            }

            Ok(Self {
                last_content: Arc::new(Mutex::new(String::new())),
            })
        }

        async fn get_clipboard_contents(&self) -> Result<String> {
            // Prefer PowerShell for getting clipboard contents as it handles Unicode better
            if is_powershell_available() {
                let output = TokioCommand::new("powershell.exe")
                    .arg("-Command")
                    .arg("Get-Clipboard")
                    .arg("-Format")
                    .arg("Text")
                    .output()
                    .await
                    .map_err(|e| PostError::Clipboard(format!("Failed to execute PowerShell Get-Clipboard: {}", e)))?;

                if !output.status.success() {
                    // Empty clipboard is not an error
                    if output.status.code() == Some(1) {
                        return Ok(String::new());
                    }
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    return Err(PostError::Clipboard(format!("PowerShell Get-Clipboard failed: {}", stderr)));
                }

                let content = String::from_utf8(output.stdout)
                    .map_err(|e| PostError::Clipboard(format!("Invalid UTF-8 in clipboard: {}", e)))?;
                    
                // Remove trailing newline that PowerShell adds
                Ok(content.trim_end().to_string())
            } else {
                // Fallback: try to use clip.exe in a roundabout way (though it's primarily for setting)
                Err(PostError::Clipboard("Cannot read from clipboard: PowerShell not available".to_string()))
            }
        }

        async fn set_clipboard_contents(&self, content: &str) -> Result<()> {
            // Use clip.exe for setting clipboard contents
            if is_clip_exe_available() {
                let mut cmd = TokioCommand::new("clip.exe")
                    .stdin(std::process::Stdio::piped())
                    .spawn()
                    .map_err(|e| PostError::Clipboard(format!("Failed to execute clip.exe: {}", e)))?;

                if let Some(stdin) = cmd.stdin.as_mut() {
                    use tokio::io::AsyncWriteExt;
                    stdin.write_all(content.as_bytes()).await.map_err(|e| {
                        PostError::Clipboard(format!("Failed to write to clip.exe: {}", e))
                    })?;
                    stdin.shutdown().await.map_err(|e| {
                        PostError::Clipboard(format!("Failed to close clip.exe stdin: {}", e))
                    })?;
                }

                let status = cmd
                    .wait()
                    .await
                    .map_err(|e| PostError::Clipboard(format!("Failed to wait for clip.exe: {}", e)))?;

                if !status.success() {
                    return Err(PostError::Clipboard(format!(
                        "clip.exe failed with exit code: {:?}",
                        status.code()
                    )));
                }

                debug!("Set WSL clipboard contents via clip.exe: {} chars", content.len());
                Ok(())
            } else if is_powershell_available() {
                // Fallback to PowerShell Set-Clipboard
                let mut cmd = TokioCommand::new("powershell.exe")
                    .arg("-Command")
                    .arg("Set-Clipboard")
                    .arg("-Value")
                    .arg(content)
                    .spawn()
                    .map_err(|e| PostError::Clipboard(format!("Failed to execute PowerShell Set-Clipboard: {}", e)))?;

                let status = cmd
                    .wait()
                    .await
                    .map_err(|e| PostError::Clipboard(format!("Failed to wait for PowerShell: {}", e)))?;

                if !status.success() {
                    return Err(PostError::Clipboard(format!(
                        "PowerShell Set-Clipboard failed with exit code: {:?}",
                        status.code()
                    )));
                }

                debug!("Set WSL clipboard contents via PowerShell: {} chars", content.len());
                Ok(())
            } else {
                Err(PostError::Clipboard("Cannot set clipboard: neither clip.exe nor PowerShell available".to_string()))
            }
        }
    }

    #[async_trait::async_trait]
    impl ClipboardManager for WSLClipboard {
        async fn get_contents(&self) -> Result<String> {
            self.get_clipboard_contents().await
        }

        async fn set_contents(&self, content: &str) -> Result<()> {
            self.set_clipboard_contents(content).await?;

            let mut last = self.last_content.lock().await;
            *last = content.to_owned();

            Ok(())
        }
    }

    #[async_trait::async_trait]
    impl ClipboardWatcher for WSLClipboard {
        async fn watch_changes(
            &self,
            callback: Box<dyn Fn(String) + Send + Sync + 'static>,
        ) -> Result<()> {
            let last_content = Arc::clone(&self.last_content);

            tokio::spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_millis(1000)); // Slightly slower for WSL

                loop {
                    interval.tick().await;

                    // Get current clipboard content
                    let current_content = if is_powershell_available() {
                        let output = TokioCommand::new("powershell.exe")
                            .arg("-Command")
                            .arg("Get-Clipboard")
                            .arg("-Format")
                            .arg("Text")
                            .output()
                            .await;

                        match output {
                            Ok(output) if output.status.success() => {
                                String::from_utf8_lossy(&output.stdout).trim_end().to_string()
                            }
                            _ => {
                                warn!("Failed to check WSL clipboard via PowerShell");
                                continue;
                            }
                        }
                    } else {
                        warn!("Cannot watch WSL clipboard: PowerShell not available");
                        continue;
                    };

                    let mut last = last_content.lock().await;
                    if current_content != *last && !current_content.is_empty() {
                        *last = current_content.clone();
                        drop(last);

                        debug!("WSL clipboard changed: {} chars", current_content.len());
                        callback(current_content);
                    }
                }
            });

            Ok(())
        }
    }
}
