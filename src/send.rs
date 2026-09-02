//! Send a message through the official KakaoTalk Windows desktop app UI.
//!
//! This is a local UI driver (UI Automation / SendInput / clipboard), not a
//! Kakao protocol client. KakaoTalk.exe must already be running and logged in.
//! macOS send is out of scope for this fork.

mod driver;
mod error;
mod target;
#[cfg(target_os = "windows")]
mod windows_ui;

pub use driver::{peek_messages, send_message, FakeUi, KakaoTalkUi, PeekBubble, UiStatus};
pub use error::SendError;
pub use target::{pick_visible_title, resolve_target, titles_match, ResolvedTarget, SendRequest};

use serde::Serialize;

/// Result of a send or dry-run. Never includes the message body.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SendReport {
    pub resolved: bool,
    pub room: String,
    pub chat_id: Option<String>,
    pub sent: bool,
    pub dry_run: bool,
    pub chars: usize,
}

/// Last visible bubbles from an already-open KakaoTalk chat window.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PeekReport {
    pub room: String,
    pub chat_id: Option<String>,
    pub bubbles: Vec<PeekBubble>,
}

/// Build the platform UI driver. Non-Windows hosts fail clearly.
pub fn platform_ui() -> Result<Box<dyn KakaoTalkUi>, SendError> {
    #[cfg(target_os = "windows")]
    {
        Ok(Box::new(windows_ui::WindowsKakaoTalkUi::connect()?))
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err(SendError::UnsupportedOs)
    }
}
