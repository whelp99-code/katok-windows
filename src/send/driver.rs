use super::error::SendError;
use super::target::{titles_match, ResolvedTarget, SendRequest};
use super::SendReport;
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
    fn focus_or_open_chat(&self, title: &str) -> Result<(), SendError>;
    fn paste_compose(&self, text: &str) -> Result<(), SendError>;
    fn press_send(&self) -> Result<(), SendError>;
}

/// In-memory KakaoTalk stand-in. Never talks to a real install.
pub struct FakeUi {
    pub status: UiStatus,
    pub open_titles: Vec<String>,
    pub searchable: Vec<String>,
    pub focused: RefCell<Option<String>>,
    pub pasted: RefCell<Option<String>>,
    pub send_presses: RefCell<usize>,
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
        }
    }

    pub fn focused(&self) -> Option<String> {
        self.focused.borrow().clone()
    }

    pub fn pasted(&self) -> Option<String> {
        self.pasted.borrow().clone()
    }

    pub fn send_presses(&self) -> usize {
        *self.send_presses.borrow()
    }

    fn matching<'a>(titles: &'a [String], wanted: &str) -> Vec<&'a str> {
        titles
            .iter()
            .filter(|title| titles_match(title, wanted))
            .map(String::as_str)
            .collect()
    }
}

impl KakaoTalkUi for FakeUi {
    fn status(&self) -> Result<UiStatus, SendError> {
        Ok(self.status.clone())
    }

    fn list_open_chat_titles(&self) -> Result<Vec<String>, SendError> {
        Ok(self.open_titles.clone())
    }

    fn focus_or_open_chat(&self, title: &str) -> Result<(), SendError> {
        let open = Self::matching(&self.open_titles, title);
        if open.len() > 1 {
            return Err(SendError::AmbiguousRoom {
                room: title.to_string(),
                ids: open.join(", "),
            });
        }
        if let Some(found) = open.first() {
            *self.focused.borrow_mut() = Some((*found).to_string());
            return Ok(());
        }
        let searchable = Self::matching(&self.searchable, title);
        if let Some(found) = searchable.first() {
            *self.focused.borrow_mut() = Some((*found).to_string());
            return Ok(());
        }
        Err(SendError::ChatNotFound(title.to_string()))
    }

    fn paste_compose(&self, text: &str) -> Result<(), SendError> {
        *self.pasted.borrow_mut() = Some(text.to_string());
        Ok(())
    }

    fn press_send(&self) -> Result<(), SendError> {
        *self.send_presses.borrow_mut() += 1;
        Ok(())
    }
}

/// Focus (and optionally send to) a resolved chat. `--dry-run` never pastes.
pub fn send_message(
    ui: &dyn KakaoTalkUi,
    request: &SendRequest,
    target: &ResolvedTarget,
) -> Result<SendReport, SendError> {
    let status = ui.status()?;
    if !status.running {
        return Err(SendError::NotRunning);
    }
    if !status.logged_in {
        return Err(SendError::NotLoggedIn);
    }

    ui.focus_or_open_chat(&target.title)?;

    if request.dry_run {
        return Ok(SendReport {
            resolved: true,
            room: target.title.clone(),
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
        room: target.title.clone(),
        chat_id: target.chat_id.clone(),
        sent: true,
        dry_run: false,
        chars: body.chars().count(),
    })
}
