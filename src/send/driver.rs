use super::error::SendError;
use super::peek::{filter_peek_bubbles, PeekBubble};
use super::target::{pick_visible_title, ResolvedTarget, SendRequest};
use super::{PeekReport, SendReport};
use std::cell::RefCell;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiStatus {
    pub running: bool,
    pub logged_in: bool,
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
    /// Current compose-box text. Empty after a real send.
    fn compose_value(&self) -> Result<String, SendError>;
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
    pub bubbles: RefCell<Vec<PeekBubble>>,
    pub compose: RefCell<Option<String>>,
    pub commit_on_send: bool,
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
            bubbles: RefCell::new(Vec::new()),
            compose: RefCell::new(None),
            commit_on_send: true,
        }
    }

    pub fn with_foreground_lock(mut self) -> Self {
        self.refuse_foreground = true;
        self
    }

    /// Paste into compose but do not commit Send (grey/idle Send button).
    pub fn with_idle_send(mut self) -> Self {
        self.commit_on_send = false;
        self
    }

    pub fn with_bubbles(mut self, bubbles: Vec<PeekBubble>) -> Self {
        self.bubbles = RefCell::new(bubbles);
        self
    }

    pub fn compose(&self) -> Option<String> {
        self.compose.borrow().clone()
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
        *self.compose.borrow_mut() = Some(text.to_string());
        Ok(())
    }

    fn press_send(&self) -> Result<(), SendError> {
        *self.send_presses.borrow_mut() += 1;
        if self.commit_on_send {
            if let Some(text) = self.compose.borrow_mut().take() {
                self.bubbles.borrow_mut().push(PeekBubble {
                    direction: "outgoing",
                    text,
                    sender: None,
                });
            }
        }
        Ok(())
    }

    fn peek_visible_bubbles(&self) -> Result<Vec<PeekBubble>, SendError> {
        Ok(self.bubbles.borrow().clone())
    }

    fn compose_value(&self) -> Result<String, SendError> {
        Ok(self.compose.borrow().clone().unwrap_or_default())
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
    let prior_outgoing = outgoing_texts(ui);
    ui.press_send()?;
    if !confirm_delivered(ui, body, &prior_outgoing)? {
        return Err(SendError::NotDelivered);
    }

    Ok(SendReport {
        resolved: true,
        room: visible,
        chat_id: target.chat_id.clone(),
        sent: true,
        dry_run: false,
        chars: body.chars().count(),
    })
}

fn outgoing_texts(ui: &dyn KakaoTalkUi) -> Vec<String> {
    filter_peek_bubbles(ui.peek_visible_bubbles().unwrap_or_default())
        .into_iter()
        .filter(|bubble| bubble.direction == "outgoing")
        .map(|bubble| bubble.text)
        .collect()
}

fn confirm_delivered(
    ui: &dyn KakaoTalkUi,
    body: &str,
    prior_outgoing: &[String],
) -> Result<bool, SendError> {
    let leftover = ui.compose_value()?.trim().to_string();
    if leftover.is_empty() {
        return Ok(true);
    }
    let now = outgoing_texts(ui);
    let prior_hits = prior_outgoing
        .iter()
        .filter(|text| text.trim() == body)
        .count();
    let now_hits = now.iter().filter(|text| text.trim() == body).count();
    Ok(now_hits > prior_hits)
}

/// Last visible bubbles from an already-open chat window. Never sends.
pub fn peek_messages(
    ui: &dyn KakaoTalkUi,
    _request: &SendRequest,
    target: &ResolvedTarget,
) -> Result<PeekReport, SendError> {
    ensure_ready(ui)?;
    let visible = ui.prepare_chat(&target.title, false)?;
    let bubbles = filter_peek_bubbles(ui.peek_visible_bubbles()?);
    Ok(PeekReport {
        room: visible,
        chat_id: target.chat_id.clone(),
        bubbles,
    })
}
