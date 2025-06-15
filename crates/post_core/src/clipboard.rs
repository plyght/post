use crate::{PostError, Result};
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
