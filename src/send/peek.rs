//! Turn visible UI nodes into chat bubbles. Shared by the fake UI tests and
//! the Windows driver so Linux can cover group-chat layout without KakaoTalk.

use serde::Serialize;

/// One visible chat bubble. `direction` is `incoming`, `outgoing`, or `unknown`.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PeekBubble {
    pub direction: &'static str,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sender: Option<String>,
}

impl PeekBubble {
    pub fn new(direction: &'static str, text: impl Into<String>) -> Self {
        Self {
            direction,
            text: text.into(),
            sender: None,
        }
    }
}

/// Axis-aligned window or control rect, in screen pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeekRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl PeekRect {
    pub fn width(self) -> i32 {
        self.right.saturating_sub(self.left)
    }

    pub fn height(self) -> i32 {
        self.bottom.saturating_sub(self.top)
    }
}

/// One accessibility node from UIA RawView, MSAA, or a child HWND.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeekNode {
    pub text: String,
    pub rect: PeekRect,
    pub is_compose: bool,
    pub is_button: bool,
}

/// Drop compose-box chrome such as `RichEdit Control`. Never keep that as a bubble.
pub fn keep_bubble_text(text: &str) -> bool {
    let text = text.trim();
    if text.is_empty() || text.chars().count() > 2000 {
        return false;
    }
    let lower = text.to_ascii_lowercase();
    if lower.contains("richedit") || lower == "edit control" || lower == "document control" {
        return false;
    }
    if looks_like_timestamp(text) || looks_like_unread_badge(text) {
        return false;
    }
    !matches!(
        text,
        "전송"
            | "보내기"
            | "Send"
            | "검색"
            | "Search"
            | "카카오톡"
            | "KakaoTalk"
            | "이모티콘"
            | "사진"
            | "파일"
            | "Enter a message"
            | "Enter Message"
            | "메시지를 입력하세요"
            | "메시지 입력"
    )
}

/// Classify visible nodes into incoming/outgoing bubbles.
///
/// Skips the compose RichEdit, Send, the top notice banner, and timestamps.
/// Does not drop ordinary left-side list/chat-pane text. Group-room names
/// sitting just above an incoming body become `sender`.
pub fn bubbles_from_nodes(nodes: &[PeekNode], window: PeekRect, compose: &str) -> Vec<PeekBubble> {
    let compose = compose.trim();
    let mid_x = (window.left + window.right) / 2;
    let compose_top = window.bottom.saturating_sub(compose_strip_height(window));
    let mut scored: Vec<(i32, i32, i32, i32, String)> = Vec::new();
    for node in nodes {
        if node.is_compose || node.is_button {
            continue;
        }
        let text = node.text.trim();
        if !keep_bubble_text(text) {
            continue;
        }
        if !compose.is_empty() && text == compose {
            continue;
        }
        if node.rect.bottom <= node.rect.top || node.rect.top >= compose_top {
            continue;
        }
        if is_notice_banner(node.rect, window) {
            continue;
        }
        scored.push((
            node.rect.top,
            node.rect.left,
            node.rect.right,
            node.rect.bottom,
            text.to_string(),
        ));
    }
    scored.sort_by_key(|(top, left, _, _, _)| (*top, *left));
    scored.dedup_by(|a, b| a.4 == b.4 && (a.0 - b.0).abs() < 8);

    let mut bubbles = Vec::new();
    let mut index = 0;
    while index < scored.len() {
        let (top, left, right, bottom, text) = scored[index].clone();
        let incoming = left < mid_x;
        if incoming {
            if let Some(next) = scored.get(index + 1) {
                if next.1 < mid_x && is_sender_label(&text, top, left, bottom, next) {
                    bubbles.push(PeekBubble {
                        direction: "incoming",
                        text: next.4.clone(),
                        sender: Some(text),
                    });
                    index += 2;
                    continue;
                }
            }
            bubbles.push(PeekBubble {
                direction: "incoming",
                text,
                sender: None,
            });
        } else if right > mid_x {
            bubbles.push(PeekBubble {
                direction: "outgoing",
                text,
                sender: None,
            });
        }
        index += 1;
    }

    const KEEP: usize = 20;
    let start = bubbles.len().saturating_sub(KEEP);
    bubbles.drain(0..start);
    bubbles
}

pub(crate) fn filter_peek_bubbles(bubbles: Vec<PeekBubble>) -> Vec<PeekBubble> {
    bubbles
        .into_iter()
        .filter(|bubble| keep_bubble_text(&bubble.text))
        .collect()
}

fn compose_strip_height(window: PeekRect) -> i32 {
    (window.height() / 10).max(72)
}

fn is_notice_banner(rect: PeekRect, window: PeekRect) -> bool {
    let width = rect.width();
    let window_width = window.width().max(1);
    width * 100 / window_width >= 85
        && rect.left - window.left <= 48
        && rect.top < window.top + window.height() / 2
}

fn is_sender_label(
    text: &str,
    top: i32,
    left: i32,
    bottom: i32,
    next: &(i32, i32, i32, i32, String),
) -> bool {
    if text.contains('\n') || text.chars().count() > 40 {
        return false;
    }
    if next.4.chars().count() <= text.chars().count() {
        return false;
    }
    let gap = next.0.saturating_sub(bottom);
    gap <= 40 && next.0 >= top && (next.1 - left).abs() < 80
}

fn looks_like_timestamp(text: &str) -> bool {
    let text = text.trim();
    if text.is_empty() || text.chars().count() > 16 {
        return false;
    }
    let lower = text.to_ascii_lowercase();
    if (lower.ends_with("am") || lower.ends_with("pm")) && lower.contains(':') {
        return true;
    }
    text.starts_with("오전") || text.starts_with("오후")
}

fn looks_like_unread_badge(text: &str) -> bool {
    let text = text.trim();
    (1..=6).contains(&text.len()) && text.bytes().all(|b| b.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::{bubbles_from_nodes, keep_bubble_text, PeekNode, PeekRect};

    fn window() -> PeekRect {
        PeekRect {
            left: 0,
            top: 0,
            right: 1000,
            bottom: 700,
        }
    }

    fn node(text: &str, left: i32, top: i32, right: i32, bottom: i32) -> PeekNode {
        PeekNode {
            text: text.to_string(),
            rect: PeekRect {
                left,
                top,
                right,
                bottom,
            },
            is_compose: false,
            is_button: false,
        }
    }

    #[test]
    fn hermes_group_yields_two_incoming_and_one_outgoing() {
        let nodes = vec![
            node(
                "윤설 비방 비난 반말 특정 장애를 비하 발언 금지..",
                12,
                56,
                988,
                108,
            ),
            node("코난쌤 한준구", 48, 128, 220, 148),
            node("오..안티그래비티 썰먹가나요", 48, 152, 420, 188),
            node("6:34 AM", 430, 168, 510, 186),
            node("차포", 48, 210, 100, 228),
            node(
                "3.8 모텔 카탈로그 떴나요? 왜 갱신 안되는거지",
                48,
                232,
                460,
                278,
            ),
            node("졸작역 고민하다가 3.8보고 신청하기로..", 620, 480, 960, 528),
            PeekNode {
                text: "RichEdit Control".into(),
                rect: PeekRect {
                    left: 8,
                    top: 630,
                    right: 820,
                    bottom: 690,
                },
                is_compose: true,
                is_button: false,
            },
            PeekNode {
                text: "Send".into(),
                rect: PeekRect {
                    left: 880,
                    top: 640,
                    right: 980,
                    bottom: 684,
                },
                is_compose: false,
                is_button: true,
            },
        ];
        let bubbles = bubbles_from_nodes(&nodes, window(), "");
        assert_eq!(bubbles.len(), 3, "{bubbles:?}");
        assert_eq!(bubbles[0].direction, "incoming");
        assert_eq!(bubbles[0].sender.as_deref(), Some("코난쌤 한준구"));
        assert_eq!(bubbles[0].text, "오..안티그래비티 썰먹가나요");
        assert_eq!(bubbles[1].direction, "incoming");
        assert_eq!(bubbles[1].sender.as_deref(), Some("차포"));
        assert!(bubbles[1].text.contains("카탈로그"));
        assert_eq!(bubbles[2].direction, "outgoing");
        assert!(bubbles[2].text.contains("졸작역"));
        assert!(bubbles[2].sender.is_none());
        assert!(bubbles
            .iter()
            .all(|bubble| !bubble.text.to_ascii_lowercase().contains("richedit")));
    }

    #[test]
    fn keep_bubble_text_rejects_compose_chrome_not_messages() {
        assert!(!keep_bubble_text("RichEdit Control"));
        assert!(!keep_bubble_text("Enter a message"));
        assert!(!keep_bubble_text("6:34 AM"));
        assert!(!keep_bubble_text("99"));
        assert!(!keep_bubble_text("1886"));
        assert!(keep_bubble_text("다시"));
        assert!(keep_bubble_text("포커스없이"));
        assert!(keep_bubble_text("전송확인"));
    }
}
