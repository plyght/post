use notify_rust::Notification;
use post_core::Result;
use std::time::Duration;
use tracing::{debug, warn};

#[derive(Clone)]
pub struct NotificationManager {
    app_name: String,
}

impl NotificationManager {
    pub fn new() -> Self {
        Self {
            app_name: "Post Clipboard Sync".to_string(),
        }
    }

    /// Show a notification that Tailscale connection was lost
    pub fn show_tailscale_disconnected(&self) -> Result<()> {
        self.show_notification(
            "Tailscale Disconnected",
            "Post clipboard sync is offline. Will retry every 2 seconds.",
        )
    }

    /// Show a notification that Tailscale connection was established
    pub fn show_tailscale_connected(&self, node_id: &str) -> Result<()> {
        self.show_notification(
            "Tailscale Connected",
            &format!("Post clipboard sync is online ({})", node_id),
        )
    }

    /// Show a notification that the daemon started without Tailscale
    pub fn show_daemon_started_offline(&self) -> Result<()> {
        self.show_notification("Post Daemon Started", "Waiting for Tailscale connection...")
    }

    fn show_notification(&self, summary: &str, body: &str) -> Result<()> {
        let result = Notification::new()
            .summary(summary)
            .body(body)
            .appname(&self.app_name)
            .timeout(Duration::from_secs(5))
            .show();

        match result {
            Ok(_) => {
                debug!("Notification shown: {}", summary);
                Ok(())
            }
            Err(e) => {
                warn!("Failed to show notification: {}", e);
                // Don't fail the daemon just because notifications don't work
                Ok(())
            }
        }
    }
}

impl Default for NotificationManager {
    fn default() -> Self {
        Self::new()
    }
}
