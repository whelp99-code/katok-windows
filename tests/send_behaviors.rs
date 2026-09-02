//! Non-UI send logic: targeting, dry-run planning, and CLI help.
//!
//! These tests never talk to a real KakaoTalk install. UI failures
//! (app not running / chat not found) are exercised against a fake driver.

use chrono::{TimeZone, Utc};
use katok::archive::Archive;
use katok::send::{
    peek_messages, pick_visible_title, send_message, titles_match, FakeUi, PeekBubble, SendError,
    SendRequest, UiStatus,
};
use katok::types::RawMessage;

fn archive_with(messages: &[RawMessage]) -> (tempfile::TempDir, Archive) {
    let dir = tempfile::tempdir().expect("tempdir");
    let archive = Archive::open(&dir.path().join("archive.sqlite3")).expect("open archive");
    archive.sync_messages(messages).expect("sync");
    (dir, archive)
}

fn message(chat_id: &str, chat_name: &str, id: &str) -> RawMessage {
    RawMessage {
        account_hash: "txt-import".to_string(),
        chat_id: chat_id.to_string(),
        chat_name: chat_name.to_string(),
        chat_type: "direct".to_string(),
        message_id: id.to_string(),
        sender_id: "s1".to_string(),
        sender_nickname: "민지".to_string(),
        timestamp: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
        text: "synthetic".to_string(),
        message_type: "text".to_string(),
        reply_to_message_id: None,
    }
}

#[test]
fn room_title_is_used_as_visible_target_without_archive() {
    let target = katok::send::resolve_target(
        &SendRequest {
            room: Some("제피란더스".to_string()),
            chat: None,
            text: Some("hello".to_string()),
            dry_run: true,
            peek: false,
        },
        None,
    )
    .expect("resolve");
    assert_eq!(target.title, "제피란더스");
    assert_eq!(target.chat_id, None);
}

#[test]
fn chat_id_resolves_to_archive_display_name() {
    let (_dir, archive) = archive_with(&[message("txt-zephyr", "제피란더스", "m1")]);
    let target = katok::send::resolve_target(
        &SendRequest {
            room: None,
            chat: Some("txt-zephyr".to_string()),
            text: None,
            dry_run: true,
            peek: false,
        },
        Some(&archive),
    )
    .expect("resolve");
    assert_eq!(target.title, "제피란더스");
    assert_eq!(target.chat_id.as_deref(), Some("txt-zephyr"));
}

#[test]
fn unknown_chat_id_fails_clearly() {
    let (_dir, archive) = archive_with(&[message("txt-zephyr", "제피란더스", "m1")]);
    let err = katok::send::resolve_target(
        &SendRequest {
            room: None,
            chat: Some("missing-chat".to_string()),
            text: None,
            dry_run: true,
            peek: false,
        },
        Some(&archive),
    )
    .expect_err("missing chat");
    let message = err.to_string();
    assert!(
        message.contains("missing-chat") && message.contains("archive"),
        "unexpected error: {message}"
    );
}

#[test]
fn ambiguous_room_name_in_archive_requires_chat_id() {
    let (_dir, archive) = archive_with(&[
        message("txt-a", "제피란더스", "m1"),
        message("txt-b", "제피란더스", "m2"),
    ]);
    let err = katok::send::resolve_target(
        &SendRequest {
            room: Some("제피란더스".to_string()),
            chat: None,
            text: None,
            dry_run: true,
            peek: false,
        },
        Some(&archive),
    )
    .expect_err("ambiguous");
    let message = err.to_string();
    assert!(
        message.contains("--chat") && message.contains("제피란더스"),
        "unexpected error: {message}"
    );
}

#[test]
fn unique_room_name_attaches_archive_chat_id() {
    let (_dir, archive) = archive_with(&[message("txt-zephyr", "제피란더스", "m1")]);
    let target = katok::send::resolve_target(
        &SendRequest {
            room: Some("제피란더스".to_string()),
            chat: None,
            text: None,
            dry_run: true,
            peek: false,
        },
        Some(&archive),
    )
    .expect("resolve");
    assert_eq!(target.chat_id.as_deref(), Some("txt-zephyr"));
}

#[test]
fn titles_match_ignores_member_order_and_whitespace() {
    assert!(titles_match("제피란더스", "제피란더스"));
    assert!(titles_match("Alpha, Beta", "Beta, Alpha"));
    assert!(titles_match("Alpha , Beta", "Beta,Alpha"));
    assert!(!titles_match("제피란더스", "제피"));
    assert!(!titles_match("Alpha, Gamma", "Beta, Alpha"));
}

#[test]
fn unique_substring_matches_parenthesized_nickname() {
    let picked = pick_visible_title(&["박재민(제피란더스)"], "제피란더스").expect("unique");
    assert_eq!(picked, "박재민(제피란더스)");
}

#[test]
fn exact_title_wins_over_substring_sibling() {
    let picked = pick_visible_title(&["제피란더스", "박재민(제피란더스)"], "제피란더스")
        .expect("exact preferred");
    assert_eq!(picked, "제피란더스");
}

#[test]
fn ambiguous_substring_does_not_guess_unrelated_rooms() {
    let err = pick_visible_title(&["제피란더스", "제피2호"], "제피").expect_err("ambiguous");
    assert!(matches!(err, SendError::AmbiguousRoom { .. }));
}

#[test]
fn dry_run_focuses_chat_but_does_not_send() {
    let ui = FakeUi::logged_in(vec!["제피란더스".into()]);
    let report = send_message(
        &ui,
        &SendRequest {
            room: Some("제피란더스".to_string()),
            chat: None,
            text: Some("이 텍스트는 보내지면 안 됨".to_string()),
            dry_run: true,
            peek: false,
        },
        &katok::send::ResolvedTarget {
            title: "제피란더스".to_string(),
            chat_id: None,
        },
    )
    .expect("dry-run");
    assert!(report.resolved);
    assert!(!report.sent);
    assert!(report.dry_run);
    assert_eq!(report.room, "제피란더스");
    assert_eq!(ui.focused().as_deref(), Some("제피란더스"));
    assert!(ui.pasted().is_none());
    assert_eq!(ui.send_presses(), 0);
}

#[test]
fn send_pastes_korean_text_then_presses_send() {
    let ui = FakeUi::logged_in(vec!["제피란더스".into()]);
    let report = send_message(
        &ui,
        &SendRequest {
            room: Some("제피란더스".to_string()),
            chat: None,
            text: Some("안녕하세요".to_string()),
            dry_run: false,
            peek: false,
        },
        &katok::send::ResolvedTarget {
            title: "제피란더스".to_string(),
            chat_id: Some("txt-zephyr".to_string()),
        },
    )
    .expect("send");
    assert!(report.sent);
    assert_eq!(ui.pasted().as_deref(), Some("안녕하세요"));
    assert_eq!(ui.send_presses(), 1);
    assert_eq!(report.chars, "안녕하세요".chars().count());
}

#[test]
fn refuses_empty_message_without_touching_compose() {
    let ui = FakeUi::logged_in(vec!["제피란더스".into()]);
    let err = send_message(
        &ui,
        &SendRequest {
            room: Some("제피란더스".to_string()),
            chat: None,
            text: Some("   \n".to_string()),
            dry_run: false,
            peek: false,
        },
        &katok::send::ResolvedTarget {
            title: "제피란더스".to_string(),
            chat_id: None,
        },
    )
    .expect_err("empty");
    assert!(matches!(err, SendError::EmptyMessage));
    assert!(ui.pasted().is_none());
    assert_eq!(ui.send_presses(), 0);
}

#[test]
fn fails_clearly_when_kakaotalk_is_not_running() {
    let ui = FakeUi {
        status: UiStatus {
            running: false,
            logged_in: false,
        },
        open_titles: vec![],
        searchable: vec![],
        focused: std::cell::RefCell::new(None),
        pasted: std::cell::RefCell::new(None),
        send_presses: std::cell::RefCell::new(0),
        refuse_foreground: false,
        prepared: std::cell::RefCell::new(None),
        bubbles: std::cell::RefCell::new(vec![]),
        compose: std::cell::RefCell::new(None),
        commit_on_send: true,
    };
    let err = send_message(
        &ui,
        &SendRequest {
            room: Some("제피란더스".to_string()),
            chat: None,
            text: Some("hi".to_string()),
            dry_run: true,
            peek: false,
        },
        &katok::send::ResolvedTarget {
            title: "제피란더스".to_string(),
            chat_id: None,
        },
    )
    .expect_err("not running");
    let message = err.to_string();
    assert!(
        message.contains("KakaoTalk.exe") && message.contains("not a login tool"),
        "unexpected error: {message}"
    );
}

#[test]
fn fails_clearly_when_chat_is_not_found() {
    let ui = FakeUi::logged_in(vec!["다른방".into()]);
    let err = send_message(
        &ui,
        &SendRequest {
            room: Some("제피란더스".to_string()),
            chat: None,
            text: None,
            dry_run: true,
            peek: false,
        },
        &katok::send::ResolvedTarget {
            title: "제피란더스".to_string(),
            chat_id: None,
        },
    )
    .expect_err("missing chat");
    let message = err.to_string();
    assert!(
        message.contains("제피란더스") && message.to_lowercase().contains("not found"),
        "unexpected error: {message}"
    );
}

#[test]
fn fails_clearly_on_login_screen() {
    let ui = FakeUi {
        status: UiStatus {
            running: true,
            logged_in: false,
        },
        open_titles: vec!["KakaoTalk".into()],
        searchable: vec![],
        focused: std::cell::RefCell::new(None),
        pasted: std::cell::RefCell::new(None),
        send_presses: std::cell::RefCell::new(0),
        refuse_foreground: false,
        prepared: std::cell::RefCell::new(None),
        bubbles: std::cell::RefCell::new(vec![]),
        compose: std::cell::RefCell::new(None),
        commit_on_send: true,
    };
    let err = send_message(
        &ui,
        &SendRequest {
            room: Some("제피란더스".to_string()),
            chat: None,
            text: None,
            dry_run: true,
            peek: false,
        },
        &katok::send::ResolvedTarget {
            title: "제피란더스".to_string(),
            chat_id: None,
        },
    )
    .expect_err("login");
    let message = err.to_string();
    assert!(
        message.contains("login") && message.contains("not a login tool"),
        "unexpected error: {message}"
    );
}

#[test]
fn send_works_when_chat_is_open_but_not_foreground() {
    let ui = FakeUi::logged_in(vec!["박재민(제피란더스)".into()]).with_foreground_lock();
    let report = send_message(
        &ui,
        &SendRequest {
            room: Some("제피란더스".to_string()),
            chat: None,
            text: Some("포커스 없이 보내기".to_string()),
            dry_run: false,
            peek: false,
        },
        &katok::send::ResolvedTarget {
            title: "제피란더스".to_string(),
            chat_id: None,
        },
    )
    .expect("unfocused send");
    assert!(report.sent);
    assert!(ui.focused().is_none());
    assert_eq!(ui.prepared().as_deref(), Some("박재민(제피란더스)"));
    assert_eq!(ui.pasted().as_deref(), Some("포커스 없이 보내기"));
    assert_eq!(ui.send_presses(), 1);
}

#[test]
fn send_fails_if_compose_still_has_text_after_send() {
    let ui = FakeUi::logged_in(vec!["박재민".into()])
        .with_foreground_lock()
        .with_idle_send();
    let err = send_message(
        &ui,
        &SendRequest {
            room: Some("박재민".to_string()),
            chat: None,
            text: Some("포커스없이".to_string()),
            dry_run: false,
            peek: false,
        },
        &katok::send::ResolvedTarget {
            title: "박재민".to_string(),
            chat_id: None,
        },
    )
    .expect_err("paste without enter");
    let message = err.to_string();
    assert!(
        message.to_lowercase().contains("compose")
            || message.to_lowercase().contains("did not send")
            || message.contains("Send"),
        "unexpected error: {message}"
    );
    assert_eq!(ui.compose().as_deref(), Some("포커스없이"));
    assert_eq!(ui.pasted().as_deref(), Some("포커스없이"));
}

#[test]
fn peek_filters_compose_richedit_control() {
    let ui = FakeUi::logged_in(vec!["박재민".into()]).with_bubbles(vec![
        PeekBubble {
            direction: "incoming",
            text: "RichEdit Control".to_string(),
        },
        PeekBubble {
            direction: "outgoing",
            text: "다시".to_string(),
        },
    ]);
    let report = peek_messages(
        &ui,
        &SendRequest {
            room: Some("박재민".to_string()),
            chat: None,
            text: None,
            dry_run: false,
            peek: true,
        },
        &katok::send::ResolvedTarget {
            title: "박재민".to_string(),
            chat_id: None,
        },
    )
    .expect("peek");
    assert!(
        report
            .bubbles
            .iter()
            .all(|bubble| !bubble.text.to_ascii_lowercase().contains("richedit")),
        "compose control leaked: {:?}",
        report.bubbles
    );
    assert_eq!(report.bubbles.len(), 1);
    assert_eq!(report.bubbles[0].text, "다시");
    assert_eq!(report.bubbles[0].direction, "outgoing");
}

#[test]
fn peek_reads_last_visible_bubbles_from_open_chat() {
    let ui = FakeUi::logged_in(vec!["박재민(제피란더스)".into()]).with_bubbles(vec![
        PeekBubble {
            direction: "incoming",
            text: "지금 가능해요?".to_string(),
        },
        PeekBubble {
            direction: "outgoing",
            text: "네".to_string(),
        },
    ]);
    let report = peek_messages(
        &ui,
        &SendRequest {
            room: Some("제피란더스".to_string()),
            chat: None,
            text: None,
            dry_run: false,
            peek: true,
        },
        &katok::send::ResolvedTarget {
            title: "제피란더스".to_string(),
            chat_id: None,
        },
    )
    .expect("peek");
    assert_eq!(report.room, "박재민(제피란더스)");
    assert_eq!(report.bubbles.len(), 2);
    assert_eq!(report.bubbles[0].direction, "incoming");
    assert_eq!(report.bubbles[0].text, "지금 가능해요?");
    assert_eq!(report.bubbles[1].direction, "outgoing");
    assert!(ui.pasted().is_none());
    assert_eq!(ui.send_presses(), 0);
}

#[test]
fn peek_fails_when_chat_window_is_not_open() {
    let ui = FakeUi::logged_in(vec!["다른방".into()]);
    let err = peek_messages(
        &ui,
        &SendRequest {
            room: Some("제피란더스".to_string()),
            chat: None,
            text: None,
            dry_run: false,
            peek: true,
        },
        &katok::send::ResolvedTarget {
            title: "제피란더스".to_string(),
            chat_id: None,
        },
    )
    .expect_err("closed");
    let message = err.to_string();
    assert!(
        message.contains("제피란더스") && message.to_lowercase().contains("not found"),
        "unexpected error: {message}"
    );
}
