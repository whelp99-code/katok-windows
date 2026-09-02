use crate::support::print_payload;
use anyhow::{Context, Result};
use katok::archive::Archive;
use katok::send::{platform_ui, resolve_target, send_message, SendRequest};
use std::io::Read;
use std::path::Path;

#[allow(clippy::too_many_arguments)]
pub(crate) fn run(
    room: Option<String>,
    chat: Option<String>,
    text: Option<String>,
    dry_run: bool,
    list_windows: bool,
    json: bool,
    archive_path: &Path,
) -> Result<()> {
    let ui = platform_ui().context("open KakaoTalk Windows UI")?;

    if list_windows {
        let titles = ui
            .list_open_chat_titles()
            .context("list KakaoTalk windows")?;
        return print_payload(json, &serde_json::json!({ "open_windows": titles }));
    }

    let archive = if archive_path.exists() {
        Some(Archive::open(archive_path).context("open archive")?)
    } else {
        None
    };

    let body = if dry_run {
        text
    } else {
        let raw = match text {
            Some(value) => value,
            None => {
                let mut buf = String::new();
                std::io::stdin()
                    .read_to_string(&mut buf)
                    .context("failed to read message body from stdin")?;
                buf
            }
        };
        Some(raw)
    };

    let request = SendRequest {
        room,
        chat,
        text: body,
        dry_run,
    };
    let target = resolve_target(&request, archive.as_ref()).context("resolve chat")?;
    let report = send_message(ui.as_ref(), &request, &target).context("drive KakaoTalk UI")?;
    print_payload(json, &report)
}
