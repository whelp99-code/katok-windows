//! Parse a KakaoTalk exported chat `.txt` file into katok's `RawMessage` model.
//!
//! This is the portable, decryption-free ingest path: on any OS a user can open
//! a chat room and use KakaoTalk's "대화 내보내기 / Export chat" to produce a
//! `.txt`, then `katok sync --source txt <file>` indexes it.
//!
//! Two on-disk layouts are handled:
//!   * PC / modern mobile: a date separator line
//!     `--------------- 2026년 1월 1일 목요일 ---------------`
//!     followed by messages `[이름] [오전 9:00] 본문`.
//!   * older mobile / iOS: self-contained lines
//!     `2026년 1월 1일 오전 9:00, 이름 : 본문`.
//!
//! Times are exported in the local timezone; KakaoTalk has no timezone metadata
//! in the text, so they are interpreted as KST (UTC+9, the user's locale) and
//! stored as UTC. This keeps chronological ordering and chunk grouping correct.
//!
//! Privacy: message text is only ever written into the local archive, never
//! logged. Parsing is line-local and never emits bodies to stderr.

use std::path::Path;

use chrono::{FixedOffset, TimeZone, Utc};
use sha2::{Digest, Sha256};

use crate::types::RawMessage;
use crate::{Error, Result};

/// KST (UTC+9). KakaoTalk exports wall-clock local time with no offset.
const KST_OFFSET_SECS: i32 = 9 * 3600;

/// Parse an exported chat file at `path`.
pub fn read_export(path: impl AsRef<Path>) -> Result<Vec<RawMessage>> {
    let path = path.as_ref();
    let raw = std::fs::read(path).map_err(Error::Io)?;
    let text = decode_text(&raw);
    let chat_name = chat_name_from(&text, path);
    let chat_id = chat_id_from(path);
    Ok(parse(&text, &chat_id, &chat_name))
}

/// Decode file bytes to a String. KakaoTalk exports UTF-8 (often with a BOM);
/// tolerate a UTF-8 BOM and fall back to lossy decoding for any stray bytes.
fn decode_text(raw: &[u8]) -> String {
    let body = raw.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(raw);
    String::from_utf8_lossy(body).into_owned()
}

/// A stable chat id derived from the file name (the export has no numeric id).
fn chat_id_from(path: &Path) -> String {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("export");
    let digest = Sha256::digest(stem.as_bytes());
    format!("txt-{:x}", u64::from_be_bytes(digest[..8].try_into().unwrap()))
}

/// Room name from the export header (`… 님과(의) 카카오톡 대화`) or file stem.
fn chat_name_from(text: &str, path: &Path) -> String {
    for line in text.lines().take(3) {
        let line = line.trim();
        for marker in [" 님과의 카카오톡 대화", " 님과 카카오톡 대화"] {
            if let Some(name) = line.strip_suffix(marker) {
                if !name.is_empty() {
                    return name.to_string();
                }
            }
        }
    }
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("export")
        .to_string()
}

/// A parsed timestamp with the date carried from the last separator line.
#[derive(Clone, Copy)]
struct YmdDate {
    year: i32,
    month: u32,
    day: u32,
}

fn to_utc(date: YmdDate, hour: u32, minute: u32) -> chrono::DateTime<Utc> {
    let offset = FixedOffset::east_opt(KST_OFFSET_SECS).expect("valid offset");
    offset
        .with_ymd_and_hms(date.year, date.month, date.day, hour, minute, 0)
        .single()
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|| Utc.timestamp_opt(0, 0).single().expect("epoch"))
}

fn parse(text: &str, chat_id: &str, chat_name: &str) -> Vec<RawMessage> {
    let mut messages: Vec<RawMessage> = Vec::new();
    let mut current_date: Option<YmdDate> = None;
    let mut seq: usize = 0;

    for line in text.lines() {
        let trimmed = line.trim_end_matches(['\r']);

        // Date separator: `--------------- 2026년 1월 1일 목요일 ---------------`
        if let Some(date) = parse_date_separator(trimmed) {
            current_date = Some(date);
            continue;
        }

        // PC / modern layout: `[이름] [오전 9:00] 본문`
        if let Some((name, hour, minute, body)) = parse_bracket_line(trimmed) {
            if let Some(date) = current_date {
                push_message(
                    &mut messages, &mut seq, chat_id, chat_name, name,
                    to_utc(date, hour, minute), body,
                );
                continue;
            }
        }

        // Older mobile layout: `2026년 1월 1일 오전 9:00, 이름 : 본문`
        if let Some((date, hour, minute, name, body)) = parse_inline_line(trimmed) {
            push_message(
                &mut messages, &mut seq, chat_id, chat_name, name,
                to_utc(date, hour, minute), body,
            );
            continue;
        }

        // Continuation of a multi-line message (only after one exists).
        if !trimmed.is_empty() {
            if let Some(last) = messages.last_mut() {
                last.text.push('\n');
                last.text.push_str(trimmed);
            }
        }
    }

    finalize_chat_type(&mut messages);
    messages
}

#[allow(clippy::too_many_arguments)]
fn push_message(
    messages: &mut Vec<RawMessage>,
    seq: &mut usize,
    chat_id: &str,
    chat_name: &str,
    sender: &str,
    timestamp: chrono::DateTime<Utc>,
    body: &str,
) {
    *seq += 1;
    messages.push(RawMessage {
        account_hash: "txt-import".to_string(),
        chat_id: chat_id.to_string(),
        chat_name: chat_name.to_string(),
        chat_type: "group".to_string(), // refined in finalize_chat_type
        message_id: format!("{chat_id}-{seq}"),
        sender_id: sender_id_for(sender),
        sender_nickname: sender.to_string(),
        timestamp,
        text: body.to_string(),
        message_type: "text".to_string(),
        reply_to_message_id: None,
    });
}

fn sender_id_for(name: &str) -> String {
    let digest = Sha256::digest(name.as_bytes());
    format!("{:x}", u64::from_be_bytes(digest[..8].try_into().unwrap()))
}

/// A 1:1 room has at most two distinct senders; otherwise it is a group.
fn finalize_chat_type(messages: &mut [RawMessage]) {
    let mut senders: Vec<&str> = messages.iter().map(|m| m.sender_nickname.as_str()).collect();
    senders.sort_unstable();
    senders.dedup();
    let chat_type = if senders.len() <= 2 { "direct" } else { "group" };
    for message in messages.iter_mut() {
        message.chat_type = chat_type.to_string();
    }
}

/// Parse `--------------- 2026년 1월 1일 목요일 ---------------` (dashes optional).
///
/// A separator carries only a date (+ weekday) and never a clock time, so a
/// line containing `:` is an inline message (`… 오전 9:00, 이름 : 본문`), not a
/// separator — reject it here so the inline parser handles it.
fn parse_date_separator(line: &str) -> Option<YmdDate> {
    if line.contains(':') {
        return None;
    }
    let inner = line.trim().trim_matches('-').trim();
    if !(inner.contains("년") && inner.contains("월") && inner.contains("일")) {
        return None;
    }
    parse_ymd_prefix(inner)
}

/// Parse a leading `YYYY년 M월 D일` from `s`.
fn parse_ymd_prefix(s: &str) -> Option<YmdDate> {
    let (year, rest) = split_number_before(s, "년")?;
    let (month, rest) = split_number_before(rest, "월")?;
    let (day, _rest) = split_number_before(rest, "일")?;
    Some(YmdDate {
        year: year as i32,
        month,
        day,
    })
}

/// Return the integer immediately preceding `sep` and the remainder after `sep`.
fn split_number_before<'a>(s: &'a str, sep: &str) -> Option<(u32, &'a str)> {
    let idx = s.find(sep)?;
    let number: u32 = s[..idx].trim().parse().ok()?;
    Some((number, &s[idx + sep.len()..]))
}

/// Parse `[이름] [오전 9:00] 본문` → (name, hour24, minute, body).
fn parse_bracket_line(line: &str) -> Option<(&str, u32, u32, &str)> {
    let line = line.strip_prefix('[')?;
    let sep = line.find("] [")?;
    let name = &line[..sep];
    let rest = &line[sep + 3..];
    let close = rest.find("] ")?;
    let time_part = &rest[..close];
    let body = &rest[close + 2..];
    let (hour, minute) = parse_kor_time(time_part)?;
    Some((name, hour, minute, body))
}

/// Parse `2026년 1월 1일 오전 9:00, 이름 : 본문`.
fn parse_inline_line(line: &str) -> Option<(YmdDate, u32, u32, &str, &str)> {
    if !line.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    let date = parse_ymd_prefix(line)?;
    // after the day, locate the time token and the `, name : body` tail.
    let day_idx = line.find("일")? + "일".len();
    let after_day = line[day_idx..].trim_start();
    let comma = after_day.find(", ")?;
    let time_part = after_day[..comma].trim();
    let (hour, minute) = parse_kor_time(time_part)?;
    let tail = &after_day[comma + 2..];
    let colon = tail.find(" : ")?;
    let name = tail[..colon].trim();
    let body = &tail[colon + 3..];
    Some((date, hour, minute, name, body))
}

/// Parse `오전 9:00` / `오후 2:30` / `9:00` into 24-hour (hour, minute).
fn parse_kor_time(s: &str) -> Option<(u32, u32)> {
    let s = s.trim();
    let (is_pm, hm) = if let Some(rest) = s.strip_prefix("오전") {
        (Some(false), rest.trim())
    } else if let Some(rest) = s.strip_prefix("오후") {
        (Some(true), rest.trim())
    } else {
        (None, s)
    };
    let (h, m) = hm.split_once(':')?;
    let mut hour: u32 = h.trim().parse().ok()?;
    let minute: u32 = m.trim().parse().ok()?;
    if minute > 59 {
        return None;
    }
    match is_pm {
        Some(false) => {
            // 오전 12:xx == 00:xx
            if hour == 12 {
                hour = 0;
            }
        }
        Some(true) => {
            // 오후 12:xx == 12:xx, otherwise +12
            if hour != 12 {
                hour += 12;
            }
        }
        None => {}
    }
    if hour > 23 {
        return None;
    }
    Some((hour, minute))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn parses_pc_export_layout() {
        let text = "친구 님과의 카카오톡 대화\n\
                    저장한 날짜 : 2026-01-01 09:00:00\n\
                    \n\
                    --------------- 2026년 1월 1일 목요일 ---------------\n\
                    [민지] [오전 9:00] 보고서 초안 올렸어요\n\
                    [민지] [오전 9:01] 검토 부탁드립니다\n\
                    이어지는 줄입니다\n\
                    [준호] [오후 2:30] 회의 전에 확인할게요\n";
        let msgs = parse(text, "txt-1", "친구");
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0].sender_nickname, "민지");
        assert_eq!(msgs[0].text, "보고서 초안 올렸어요");
        // multi-line continuation appended to the second message
        assert_eq!(msgs[1].text, "검토 부탁드립니다\n이어지는 줄입니다");
        // 오후 2:30 → 14:30 KST → 05:30 UTC
        assert_eq!(msgs[2].timestamp.to_rfc3339(), "2026-01-01T05:30:00+00:00");
        // two senders → direct
        assert_eq!(msgs[0].chat_type, "direct");
    }

    #[test]
    fn parses_inline_mobile_layout() {
        let text = "2026년 1월 1일 오전 9:00, 민지 : 안녕하세요\n\
                    2026년 1월 1일 오후 12:05, 준호 : 점심 뭐 먹을까\n";
        let msgs = parse(text, "txt-2", "room");
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].sender_nickname, "민지");
        assert_eq!(msgs[0].text, "안녕하세요");
        // 오후 12:05 stays 12:05 KST → 03:05 UTC
        assert_eq!(msgs[1].timestamp.to_rfc3339(), "2026-01-01T03:05:00+00:00");
    }

    #[test]
    fn korean_time_conversions() {
        assert_eq!(parse_kor_time("오전 12:00"), Some((0, 0)));
        assert_eq!(parse_kor_time("오전 9:05"), Some((9, 5)));
        assert_eq!(parse_kor_time("오후 12:30"), Some((12, 30)));
        assert_eq!(parse_kor_time("오후 2:30"), Some((14, 30)));
        assert_eq!(parse_kor_time("23:59"), Some((23, 59)));
        assert_eq!(parse_kor_time("bad"), None);
    }

    #[test]
    fn detects_date_separator() {
        let d = parse_date_separator("--------------- 2026년 1월 1일 목요일 ---------------")
            .expect("date");
        assert_eq!((d.year, d.month, d.day), (2026, 1, 1));
        assert!(parse_date_separator("[민지] [오전 9:00] hi").is_none());
    }

    #[test]
    fn chat_name_prefers_header() {
        let text = "홍길동 님과의 카카오톡 대화\n저장한 날짜 : x\n";
        assert_eq!(chat_name_from(text, &PathBuf::from("a.txt")), "홍길동");
        let text2 = "no header here\n";
        assert_eq!(chat_name_from(text2, &PathBuf::from("myroom.txt")), "myroom");
    }
}
