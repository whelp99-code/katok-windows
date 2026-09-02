//! Drive official KakaoTalk.exe through Win32 windowing and SendInput.
//!
//! No Kakao protocol, no packet impersonation: this only finds windows of the
//! already-running desktop app, focuses a chat, pastes into compose, and
//! presses Enter. Korean text is pasted as Unicode via the clipboard.

use super::driver::{KakaoTalkUi, UiStatus};
use super::error::SendError;
use super::target::{is_main_or_utility_title, looks_like_login_title, titles_match};
use std::ffi::c_void;
use std::mem::{size_of, zeroed};
use std::thread;
use std::time::Duration;
use windows::Win32::Foundation::{CloseHandle, BOOL, HANDLE, HWND, LPARAM, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, SetCursorPos, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT,
    KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEINPUT,
    MOUSE_EVENT_FLAGS, VIRTUAL_KEY, VK_CONTROL, VK_ESCAPE, VK_RETURN,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClientRect, GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId,
    IsIconic, IsWindowVisible, SendMessageW, SetForegroundWindow, ShowWindow, SW_RESTORE, WM_NULL,
};

const PROCESS_NAME: &str = "kakaotalk.exe";
const OPEN_WAIT: Duration = Duration::from_millis(150);
const OPEN_TRIES: usize = 16;
/// Win32 `CF_UNICODETEXT`. Kept as a literal so we do not pull in the Ole feature.
const CF_UNICODETEXT: u32 = 13;

#[derive(Clone)]
struct FoundWindow {
    hwnd: isize,
    title: String,
}

pub(super) struct WindowsKakaoTalkUi {
    pids: Vec<u32>,
}

impl WindowsKakaoTalkUi {
    pub(super) fn connect() -> Result<Self, SendError> {
        let pids = kakaotalk_pids()?;
        if pids.is_empty() {
            return Err(SendError::NotRunning);
        }
        Ok(Self { pids })
    }

    fn windows(&self) -> Result<Vec<FoundWindow>, SendError> {
        enumerate_process_windows(&self.pids)
    }

    fn chat_windows(&self) -> Result<Vec<FoundWindow>, SendError> {
        Ok(self
            .windows()?
            .into_iter()
            .filter(|window| !is_main_or_utility_title(&window.title))
            .collect())
    }

    fn main_window(&self) -> Result<FoundWindow, SendError> {
        let windows = self.windows()?;
        windows
            .iter()
            .find(|window| {
                let title = window.title.trim();
                title.eq_ignore_ascii_case("KakaoTalk") || title == "카카오톡"
            })
            .cloned()
            .or_else(|| windows.into_iter().next())
            .ok_or(SendError::NotRunning)
    }

    fn focus_window(&self, window: &FoundWindow) -> Result<(), SendError> {
        focus_hwnd(hwnd_from_stored(window.hwnd))
    }

    fn open_via_search(&self, title: &str) -> Result<FoundWindow, SendError> {
        let main = self.main_window()?;
        self.focus_window(&main)?;
        thread::sleep(OPEN_WAIT);
        tap_key(VK_ESCAPE)?;
        thread::sleep(Duration::from_millis(80));
        tap_combo(VK_CONTROL, VIRTUAL_KEY(0x46))?; // Ctrl+F
        thread::sleep(OPEN_WAIT);
        set_clipboard_text(title)?;
        tap_combo(VK_CONTROL, VIRTUAL_KEY(0x56))?; // Ctrl+V
        thread::sleep(Duration::from_millis(250));
        tap_key(VK_RETURN)?;

        for _ in 0..OPEN_TRIES {
            thread::sleep(OPEN_WAIT);
            if let Some(found) = self.find_chat(title)? {
                return Ok(found);
            }
        }
        if self
            .windows()?
            .iter()
            .any(|window| looks_like_login_title(&window.title))
        {
            return Err(SendError::NotLoggedIn);
        }
        Err(SendError::ChatNotFound(title.to_string()))
    }

    fn find_chat(&self, title: &str) -> Result<Option<FoundWindow>, SendError> {
        let hits: Vec<FoundWindow> = self
            .chat_windows()?
            .into_iter()
            .filter(|window| titles_match(&window.title, title))
            .collect();
        match hits.len() {
            0 => Ok(None),
            1 => Ok(hits.into_iter().next()),
            _ => Err(SendError::AmbiguousRoom {
                room: title.to_string(),
                ids: hits
                    .iter()
                    .map(|window| window.title.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
            }),
        }
    }
}

impl KakaoTalkUi for WindowsKakaoTalkUi {
    fn status(&self) -> Result<UiStatus, SendError> {
        if self.pids.is_empty() {
            return Ok(UiStatus {
                running: false,
                logged_in: false,
            });
        }
        let windows = self.windows()?;
        if windows.is_empty() {
            return Ok(UiStatus {
                running: true,
                logged_in: false,
            });
        }
        // A lone main window is not proof of the login screen — the user may
        // simply have no chat popped out. Title keywords are the hard signal.
        let logged_in = !windows
            .iter()
            .any(|window| looks_like_login_title(&window.title));
        Ok(UiStatus {
            running: true,
            logged_in,
        })
    }

    fn list_open_chat_titles(&self) -> Result<Vec<String>, SendError> {
        Ok(self
            .chat_windows()?
            .into_iter()
            .map(|window| window.title)
            .collect())
    }

    fn focus_or_open_chat(&self, title: &str) -> Result<(), SendError> {
        if let Some(found) = self.find_chat(title)? {
            return self.focus_window(&found);
        }
        let opened = self.open_via_search(title)?;
        self.focus_window(&opened)
    }

    fn paste_compose(&self, text: &str) -> Result<(), SendError> {
        let hwnd = unsafe { GetForegroundWindow() };
        if hwnd.is_invalid() {
            return Err(SendError::Ui(
                "could not find the foreground KakaoTalk chat window".into(),
            ));
        }
        click_bottom_center(hwnd)?;
        thread::sleep(Duration::from_millis(80));
        set_clipboard_text(text)?;
        tap_combo(VK_CONTROL, VIRTUAL_KEY(0x56))?;
        thread::sleep(Duration::from_millis(80));
        Ok(())
    }

    fn press_send(&self) -> Result<(), SendError> {
        tap_key(VK_RETURN)
    }
}

fn kakaotalk_pids() -> Result<Vec<u32>, SendError> {
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)
            .map_err(|err| SendError::Ui(err.to_string()))?;
        let mut entry = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            ..zeroed()
        };
        let mut pids = Vec::new();
        if Process32FirstW(snap, &mut entry).is_ok() {
            loop {
                let name = wchar_to_string(&entry.szExeFile);
                if name.eq_ignore_ascii_case(PROCESS_NAME) {
                    pids.push(entry.th32ProcessID);
                }
                if Process32NextW(snap, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
        Ok(pids)
    }
}

fn enumerate_process_windows(pids: &[u32]) -> Result<Vec<FoundWindow>, SendError> {
    let mut found = Vec::new();
    let mut state = EnumState {
        pids: pids.to_vec(),
        found: &mut found,
    };
    unsafe {
        EnumWindows(
            Some(enum_windows_proc),
            LPARAM(&mut state as *mut EnumState as isize),
        )
        .map_err(|err| SendError::Ui(err.to_string()))?;
    }
    Ok(found)
}

struct EnumState<'a> {
    pids: Vec<u32>,
    found: &'a mut Vec<FoundWindow>,
}

unsafe extern "system" fn enum_windows_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let state = &mut *(lparam.0 as *mut EnumState);
    if !is_true(IsWindowVisible(hwnd)) {
        return BOOL(1);
    }
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    if !state.pids.contains(&pid) {
        return BOOL(1);
    }
    let title = window_title(hwnd);
    if title.is_empty() {
        return BOOL(1);
    }
    state.found.push(FoundWindow {
        hwnd: hwnd.0 as isize,
        title,
    });
    BOOL(1)
}

fn window_title(hwnd: HWND) -> String {
    let mut buf = [0u16; 512];
    let len = unsafe { GetWindowTextW(hwnd, &mut buf) };
    if len <= 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buf[..len as usize])
}

fn wchar_to_string(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

fn hwnd_from_stored(hwnd: isize) -> HWND {
    HWND(hwnd as *mut c_void)
}

fn is_true(value: BOOL) -> bool {
    value.0 != 0
}

fn focus_hwnd(hwnd: HWND) -> Result<(), SendError> {
    unsafe {
        if is_true(IsIconic(hwnd)) {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        let foreground = GetForegroundWindow();
        let mut fg_pid = 0u32;
        let fg_tid = GetWindowThreadProcessId(foreground, Some(&mut fg_pid));
        let current = GetCurrentThreadId();
        let attached = fg_tid != 0 && is_true(AttachThreadInput(current, fg_tid, BOOL(1)));
        let ok = is_true(SetForegroundWindow(hwnd));
        if attached {
            let _ = AttachThreadInput(current, fg_tid, BOOL(0));
        }
        let _ = SendMessageW(hwnd, WM_NULL, WPARAM(0), LPARAM(0));
        if !ok {
            return Err(SendError::Ui(
                "could not focus the KakaoTalk window; click it once and retry".into(),
            ));
        }
    }
    Ok(())
}

fn click_bottom_center(hwnd: HWND) -> Result<(), SendError> {
    unsafe {
        let mut rect = RECT::default();
        GetClientRect(hwnd, &mut rect)
            .map_err(|err| SendError::Ui(format!("GetClientRect: {err}")))?;
        let mut point = POINT {
            x: (rect.right - rect.left) / 2,
            y: (rect.bottom - rect.top).saturating_sub(36),
        };
        if point.y < 8 {
            point.y = 8;
        }
        let _ = ClientToScreen(hwnd, &mut point);
        SetCursorPos(point.x, point.y)
            .map_err(|err| SendError::Ui(format!("SetCursorPos: {err}")))?;
    }
    mouse_click()
}

fn mouse_click() -> Result<(), SendError> {
    send_inputs(&[
        mouse_input(MOUSEEVENTF_LEFTDOWN),
        mouse_input(MOUSEEVENTF_LEFTUP),
    ])
}

fn mouse_input(flags: MOUSE_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: 0,
                dy: 0,
                mouseData: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn tap_key(key: VIRTUAL_KEY) -> Result<(), SendError> {
    send_inputs(&[key_input(key, false), key_input(key, true)])
}

fn tap_combo(modifier: VIRTUAL_KEY, key: VIRTUAL_KEY) -> Result<(), SendError> {
    send_inputs(&[
        key_input(modifier, false),
        key_input(key, false),
        key_input(key, true),
        key_input(modifier, true),
    ])
}

fn key_input(key: VIRTUAL_KEY, up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: key,
                wScan: 0,
                dwFlags: if up {
                    KEYEVENTF_KEYUP
                } else {
                    KEYBD_EVENT_FLAGS(0)
                },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn send_inputs(inputs: &[INPUT]) -> Result<(), SendError> {
    let sent = unsafe { SendInput(inputs, size_of::<INPUT>() as i32) };
    if sent as usize != inputs.len() {
        return Err(SendError::Ui(
            "SendInput did not deliver every event".into(),
        ));
    }
    Ok(())
}

fn set_clipboard_text(text: &str) -> Result<(), SendError> {
    let mut wide: Vec<u16> = text.encode_utf16().collect();
    wide.push(0);
    unsafe {
        OpenClipboard(HWND::default())
            .map_err(|err| SendError::Ui(format!("OpenClipboard: {err}")))?;
        let result = (|| -> Result<(), SendError> {
            EmptyClipboard().map_err(|err| SendError::Ui(format!("EmptyClipboard: {err}")))?;
            let bytes = wide.len() * size_of::<u16>();
            let handle = GlobalAlloc(GMEM_MOVEABLE, bytes)
                .map_err(|err| SendError::Ui(format!("GlobalAlloc: {err}")))?;
            let ptr = GlobalLock(handle);
            if ptr.is_null() {
                return Err(SendError::Ui("GlobalLock failed".into()));
            }
            std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr as *mut u16, wide.len());
            let _ = GlobalUnlock(handle);
            SetClipboardData(CF_UNICODETEXT, HANDLE(handle.0))
                .map_err(|err| SendError::Ui(format!("SetClipboardData: {err}")))?;
            Ok(())
        })();
        let _ = CloseClipboard();
        result
    }
}
