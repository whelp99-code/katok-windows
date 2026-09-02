use super::error::SendError;
use super::target::{pick_visible_title, ResolvedTarget, SendRequest};
use super::{PeekReport, SendReport};
use serde::Serialize;
use std::cell::RefCell;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiStatus {
    pub running: bool,
    pub logged_in: bool,
}

/// One visible chat bubble. `direction` is `incoming`, `outgoing`, or `unknown`.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PeekBubble {
    pub direction: &'static str,
    pub text: String,
}

/// Local UI surface of the official KakaoTalk.exe process.
pub trait KakaoTalkUi {
    fn status(&self) -> Result<UiStatus, SendError>;
    fn list_open_chat_titles(&self) -> Result<Vec<String>, SendError>;
    /// Resolve a chat. `allow_open` may search/open; peek passes false.
    ///
    /// Must succeed for an already-open window even when the OS refuses
    /// SetForegroundWindow. Returns the visible window title.
    fn prepare_chat(&self, title: &str, allow_open: bool) -> Result<String, SendError>;
    fn paste_compose(&self, text: &str) -> Result<(), SendError>;
    fn press_send(&self) -> Result<(), SendError>;
    fn peek_visible_bubbles(&self) -> Result<Vec<PeekBubble>, SendError>;
}

/// In-memory KakaoTalk stand-in. Never talks to a real install.
pub struct FakeUi {
    pub status: UiStatus,
    pub open_titles: Vec<String>,
    pub searchable: Vec<String>,
    pub focused: RefCell<Option<String>>,
    pub pasted: RefCell<Option<String>>,
    pub send_presses: RefCell<usize>,
    pub refuse_foreground: bool,
    pub prepared: RefCell<Option<String>>,
    pub bubbles: Vec<PeekBubble>,
}

impl FakeUi {
    pub fn logged_in(open_titles: Vec<String>) -> Self {
        Self {
            status: UiStatus {
                running: true,
                logged_in: true,
            },
            searchable: open_titles.clone(),
            open_titles,
            focused: RefCell::new(None),
            pasted: RefCell::new(None),
            send_presses: RefCell::new(0),
            refuse_foreground: false,
            prepared: RefCell::new(None),
            bubbles: Vec::new(),
        }
    }

    pub fn with_foreground_lock(mut self) -> Self {
        self.refuse_foreground = true;
        self
    }

    pub fn with_bubbles(mut self, bubbles: Vec<PeekBubble>) -> Self {
        self.bubbles = bubbles;
        self
    }

    pub fn focused(&self) -> Option<String> {
        self.focused.borrow().clone()
    }

    pub fn prepared(&self) -> Option<String> {
        self.prepared.borrow().clone()
    }

    pub fn pasted(&self) -> Option<String> {
        self.pasted.borrow().clone()
    }

    pub fn send_presses(&self) -> usize {
        *self.send_presses.borrow()
    }

    fn pick(&self, titles: &[String], wanted: &str) -> Result<String, SendError> {
        let refs: Vec<&str> = titles.iter().map(String::as_str).collect();
        pick_visible_title(refs, wanted)
    }
}

impl KakaoTalkUi for FakeUi {
    fn status(&self) -> Result<UiStatus, SendError> {
        Ok(self.status.clone())
    }

    fn list_open_chat_titles(&self) -> Result<Vec<String>, SendError> {
        Ok(self.open_titles.clone())
    }

    fn prepare_chat(&self, title: &str, allow_open: bool) -> Result<String, SendError> {
        let visible = match self.pick(&self.open_titles, title) {
            Ok(found) => found,
            Err(SendError::ChatNotFound(_)) if allow_open => self.pick(&self.searchable, title)?,
            Err(err) => return Err(err),
        };
        *self.prepared.borrow_mut() = Some(visible.clone());
        if !self.refuse_foreground {
            *self.focused.borrow_mut() = Some(visible.clone());
        }
        Ok(visible)
    }

    fn paste_compose(&self, text: &str) -> Result<(), SendError> {
        if self.prepared.borrow().is_none() {
            return Err(SendError::Ui("no chat prepared".into()));
        }
        *self.pasted.borrow_mut() = Some(text.to_string());
        Ok(())
    }

    fn press_send(&self) -> Result<(), SendError> {
        *self.send_presses.borrow_mut() += 1;
        Ok(())
    }

    fn peek_visible_bubbles(&self) -> Result<Vec<PeekBubble>, SendError> {
        Ok(self.bubbles.clone())
    }
}

fn ensure_ready(ui: &dyn KakaoTalkUi) -> Result<(), SendError> {
    let status = ui.status()?;
    if !status.running {
        return Err(SendError::NotRunning);
    }
    if !status.logged_in {
        return Err(SendError::NotLoggedIn);
    }
    Ok(())
}

/// Prepare (and optionally send to) a resolved chat. `--dry-run` never pastes.
pub fn send_message(
    ui: &dyn KakaoTalkUi,
    request: &SendRequest,
    target: &ResolvedTarget,
) -> Result<SendReport, SendError> {
    ensure_ready(ui)?;
    let visible = ui.prepare_chat(&target.title, true)?;

    if request.dry_run {
        return Ok(SendReport {
            resolved: true,
            room: visible,
            chat_id: target.chat_id.clone(),
            sent: false,
            dry_run: true,
            chars: 0,
        });
    }

    let body = request
        .text
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .ok_or(SendError::EmptyMessage)?;

    ui.paste_compose(body)?;
    ui.press_send()?;

    Ok(SendReport {
        resolved: true,
        room: visible,
        chat_id: target.chat_id.clone(),
        sent: true,
        dry_run: false,
        chars: body.chars().count(),
    })
}

/// Read last visible bubbles from an already-open chat window. Never sends.
pub fn peek_messages(
    ui: &dyn KakaoTalkUi,
    _request: &SendRequest,
    target: &ResolvedTarget,
) -> Result<PeekReport, SendError> {
    ensure_ready(ui)?;
    let visible = ui.prepare_chat(&target.title, false)?;
    let bubbles = ui.peek_visible_bubbles()?;
    Ok(PeekReport {
        room: visible,
        chat_id: target.chat_id.clone(),
        bubbles,
    })
}
