use super::error::SendError;
use crate::archive::{Archive, ChatRecord};

/// CLI-facing send request. The body is never logged by this module.
#[derive(Debug, Clone)]
pub struct SendRequest {
    pub room: Option<String>,
    pub chat: Option<String>,
    pub text: Option<String>,
    pub dry_run: bool,
}

/// Visible KakaoTalk title plus optional archive id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedTarget {
    pub title: String,
    pub chat_id: Option<String>,
}

/// Resolve `--room` / `--chat` against an optional local archive.
///
/// `--room` is the visible 1:1 title. An archive is optional and only used to
/// attach a `chat_id` or to refuse an ambiguous name. `--chat` requires the
/// archive and uses that row's `chat_name` as the UI title.
pub fn resolve_target(
    request: &SendRequest,
    archive: Option<&Archive>,
) -> Result<ResolvedTarget, SendError> {
    match (&request.chat, &request.room) {
        (Some(chat_id), _) => {
            let archive = archive.ok_or(SendError::ArchiveMissing)?;
            let row = archive
                .chat_by_id(chat_id)
                .map_err(|err| SendError::Ui(err.to_string()))?
                .ok_or_else(|| SendError::ChatNotInArchive(chat_id.clone()))?;
            Ok(ResolvedTarget {
                title: row.chat_name,
                chat_id: Some(row.chat_id),
            })
        }
        (None, Some(room)) => {
            let title = room.trim();
            if title.is_empty() {
                return Err(SendError::MissingTarget);
            }
            let chat_id = match archive {
                Some(archive) => unique_archive_id(archive, title)?,
                None => None,
            };
            Ok(ResolvedTarget {
                title: title.to_string(),
                chat_id,
            })
        }
        (None, None) => Err(SendError::MissingTarget),
    }
}

fn unique_archive_id(archive: &Archive, title: &str) -> Result<Option<String>, SendError> {
    let chats = archive
        .all_chats()
        .map_err(|err| SendError::Ui(err.to_string()))?;
    let hits: Vec<&ChatRecord> = chats
        .iter()
        .filter(|chat| titles_match(&chat.chat_name, title))
        .collect();
    match hits.len() {
        0 => Ok(None),
        1 => Ok(Some(hits[0].chat_id.clone())),
        _ => Err(SendError::AmbiguousRoom {
            room: title.to_string(),
            ids: hits
                .iter()
                .map(|chat| chat.chat_id.as_str())
                .collect::<Vec<_>>()
                .join(", "),
        }),
    }
}

/// Whether a visible KakaoTalk title is the intended room.
///
/// Named rooms compare as themselves. Untitled group rooms that list members
/// (`A, B`) match regardless of member order, matching macOS katok send.
pub fn titles_match(visible: &str, wanted: &str) -> bool {
    let left = visible.trim();
    let right = wanted.trim();
    if left == right {
        return true;
    }
    room_member_key(left) == room_member_key(right) && left.contains(',')
}

fn room_member_key(title: &str) -> Vec<String> {
    let mut parts: Vec<String> = title
        .split(',')
        .map(|part| part.trim().to_string())
        .filter(|part| !part.is_empty())
        .collect();
    parts.sort();
    parts
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(super) fn is_main_or_utility_title(title: &str) -> bool {
    let title = title.trim();
    if title.is_empty() {
        return true;
    }
    let lower = title.to_ascii_lowercase();
    matches!(
        title,
        "KakaoTalk" | "카카오톡" | "KakaoTalk Update" | "KakaoTalkSetup"
    ) || lower.contains("kakaotalk update")
        || title.contains("카카오톡 업데이트")
        || lower.contains("kakaotalkupdate")
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(super) fn looks_like_login_title(title: &str) -> bool {
    let title = title.trim();
    let lower = title.to_ascii_lowercase();
    title.contains("로그인")
        || lower == "login"
        || lower.starts_with("login ")
        || title.contains("카카오계정")
        || lower.contains("qr code")
        || title.contains("QR코드")
}

#[cfg(test)]
mod tests {
    use super::{is_main_or_utility_title, looks_like_login_title, titles_match};

    #[test]
    fn login_titles_do_not_match_ordinary_rooms() {
        assert!(looks_like_login_title("로그인"));
        assert!(looks_like_login_title("QR code login"));
        assert!(!looks_like_login_title("제피란더스"));
        assert!(!looks_like_login_title("QR연구소"));
    }

    #[test]
    fn main_kakaotalk_window_is_not_a_chat() {
        assert!(is_main_or_utility_title("KakaoTalk"));
        assert!(is_main_or_utility_title("카카오톡"));
        assert!(is_main_or_utility_title("KakaoTalk Update"));
        assert!(!is_main_or_utility_title("제피란더스"));
    }

    #[test]
    fn named_room_is_exact() {
        assert!(titles_match("제피란더스", "제피란더스"));
        assert!(!titles_match("제피란더스", "제피"));
    }
}
