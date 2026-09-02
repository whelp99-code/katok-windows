//! Drive official KakaoTalk.exe through UI Automation and RichEdit messages.
//!
//! ValuePattern SetValue does not notify KakaoTalk (Send stays grey). Paste and
//! Enter go to the RichEdit HWND. Foreground steal is best-effort only.

use super::driver::{keep_bubble_text, KakaoTalkUi, PeekBubble, UiStatus};
use super::error::SendError;
use super::target::{is_main_or_utility_title, looks_like_login_title, pick_visible_title};
use std::cell::RefCell;
use std::ffi::c_void;
use std::mem::{size_of, zeroed};
use std::thread;
use std::time::Duration;
use windows::core::{Interface, BSTR, VARIANT};
use windows::Win32::Foundation::{CloseHandle, BOOL, HANDLE, HWND, LPARAM, RECT, WPARAM};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationElementArray,
    IUIAutomationInvokePattern, IUIAutomationTextPattern, IUIAutomationValuePattern,
    TreeScope_Descendants, UIA_ButtonControlTypeId, UIA_ControlTypePropertyId,
    UIA_DocumentControlTypeId, UIA_EditControlTypeId, UIA_InvokePatternId,
    UIA_LegacyIAccessibleNamePropertyId, UIA_NamePropertyId, UIA_TextPatternId, UIA_ValuePatternId,
    UIA_PROPERTY_ID,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
    VIRTUAL_KEY, VK_CONTROL, VK_ESCAPE, VK_MENU, VK_RETURN,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, BringWindowToTop, EnumChildWindows, EnumWindows, GetClassNameW,
    GetForegroundWindow, GetWindowRect, GetWindowTextW, GetWindowThreadProcessId, IsIconic,
    IsWindowVisible, LockSetForegroundWindow, PostMessageW, SendMessageW, SetForegroundWindow,
    ShowWindow, ASFW_ANY, LSFW_UNLOCK, SW_RESTORE, WM_CHAR, WM_GETTEXT, WM_GETTEXTLENGTH,
    WM_KEYDOWN, WM_KEYUP, WM_NULL,
};

const PROCESS_NAME: &str = "kakaotalk.exe";
const OPEN_WAIT: Duration = Duration::from_millis(150);
const OPEN_TRIES: usize = 16;
const CF_UNICODETEXT: u32 = 13;
const WM_PASTE: u32 = 0x0302;
const EM_SETSEL: u32 = 0x00B1;
const EM_REPLACESEL: u32 = 0x00C2;

#[derive(Clone)]
struct FoundWindow {
    hwnd: isize,
    title: String,
}

pub(super) struct WindowsKakaoTalkUi {
    pids: Vec<u32>,
    current: RefCell<Option<FoundWindow>>,
    compose: RefCell<Option<isize>>,
}

impl WindowsKakaoTalkUi {
    pub(super) fn connect() -> Result<Self, SendError> {
        let pids = kakaotalk_pids()?;
        if pids.is_empty() {
            return Err(SendError::NotRunning);
        }
        Ok(Self {
            pids,
            current: RefCell::new(None),
            compose: RefCell::new(None),
        })
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

    fn current_hwnd(&self) -> Result<HWND, SendError> {
        let current = self.current.borrow();
        let window = current
            .as_ref()
            .ok_or_else(|| SendError::Ui("no KakaoTalk chat window prepared".into()))?;
        Ok(hwnd_from_stored(window.hwnd))
    }

    fn open_via_search(&self, title: &str) -> Result<FoundWindow, SendError> {
        let main = self.main_window()?;
        let _ = try_focus_hwnd(hwnd_from_stored(main.hwnd));
        thread::sleep(OPEN_WAIT);
        tap_key(VK_ESCAPE)?;
        thread::sleep(Duration::from_millis(80));
        tap_combo(VK_CONTROL, VIRTUAL_KEY(0x46))?;
        thread::sleep(OPEN_WAIT);
        set_clipboard_text(title)?;
        tap_combo(VK_CONTROL, VIRTUAL_KEY(0x56))?;
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
        let chats = self.chat_windows()?;
        let names: Vec<&str> = chats.iter().map(|window| window.title.as_str()).collect();
        match pick_visible_title(names, title) {
            Ok(visible) => Ok(chats.into_iter().find(|window| window.title == visible)),
            Err(SendError::ChatNotFound(_)) => Ok(None),
            Err(err) => Err(err),
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

    fn prepare_chat(&self, title: &str, allow_open: bool) -> Result<String, SendError> {
        let found = if let Some(found) = self.find_chat(title)? {
            found
        } else if allow_open {
            self.open_via_search(title)?
        } else {
            return Err(SendError::ChatNotFound(title.to_string()));
        };
        let _ = try_focus_hwnd(hwnd_from_stored(found.hwnd));
        let visible = found.title.clone();
        *self.current.borrow_mut() = Some(found);
        *self.compose.borrow_mut() = None;
        Ok(visible)
    }

    fn paste_compose(&self, text: &str) -> Result<(), SendError> {
        let hwnd = self.current_hwnd()?;
        let edit = find_compose_hwnd(hwnd)?;
        *self.compose.borrow_mut() = Some(edit.0 as isize);
        paste_into_richedit(hwnd, edit, text)?;
        Ok(())
    }

    fn press_send(&self) -> Result<(), SendError> {
        let hwnd = self.current_hwnd()?;
        let edit = self
            .compose
            .borrow()
            .map(hwnd_from_stored)
            .filter(|handle| !handle.is_invalid())
            .or_else(|| find_compose_hwnd(hwnd).ok());
        for _ in 0..8 {
            if uia_invoke_send_if_enabled(hwnd).is_ok() {
                thread::sleep(Duration::from_millis(80));
                if compose_text(edit).trim().is_empty() {
                    return Ok(());
                }
            }
            if let Some(edit) = edit {
                post_return_to_edit(edit);
                thread::sleep(Duration::from_millis(80));
                if compose_text(Some(edit)).trim().is_empty() {
                    return Ok(());
                }
            }
            thread::sleep(Duration::from_millis(40));
        }
        Ok(())
    }

    fn peek_visible_bubbles(&self) -> Result<Vec<PeekBubble>, SendError> {
        let hwnd = self.current_hwnd()?;
        let compose = compose_text(
            self.compose
                .borrow()
                .map(hwnd_from_stored)
                .or_else(|| find_compose_hwnd(hwnd).ok()),
        );
        uia_peek_bubbles(hwnd, compose.trim())
    }

    fn compose_value(&self) -> Result<String, SendError> {
        let hwnd = self.current_hwnd()?;
        let edit = self
            .compose
            .borrow()
            .map(hwnd_from_stored)
            .filter(|handle| !handle.is_invalid())
            .or_else(|| find_compose_hwnd(hwnd).ok());
        Ok(compose_text(edit))
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
    GetWindowThreadProcessId(hwnd, Some(&mut pid as *mut u32));
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

/// Best-effort focus. A false return is not fatal — UIA can still drive the HWND.
fn try_focus_hwnd(hwnd: HWND) -> bool {
    unsafe {
        if is_true(IsIconic(hwnd)) {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        let _ = AllowSetForegroundWindow(ASFW_ANY);
        let _ = LockSetForegroundWindow(LSFW_UNLOCK);
        let foreground = GetForegroundWindow();
        let mut fg_pid = 0u32;
        let fg_tid = GetWindowThreadProcessId(foreground, Some(&mut fg_pid as *mut u32));
        let mut target_pid = 0u32;
        let target_tid = GetWindowThreadProcessId(hwnd, Some(&mut target_pid as *mut u32));
        let current = GetCurrentThreadId();
        let attached_fg = fg_tid != 0 && is_true(AttachThreadInput(current, fg_tid, BOOL(1)));
        let attached_tg =
            target_tid != 0 && is_true(AttachThreadInput(current, target_tid, BOOL(1)));
        let _ = tap_key(VK_MENU);
        let _ = BringWindowToTop(hwnd);
        let ok = is_true(SetForegroundWindow(hwnd));
        if attached_fg {
            let _ = AttachThreadInput(current, fg_tid, BOOL(0));
        }
        if attached_tg {
            let _ = AttachThreadInput(current, target_tid, BOOL(0));
        }
        let _ = SendMessageW(hwnd, WM_NULL, WPARAM(0), LPARAM(0));
        ok
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

fn ensure_com() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
}

fn automation() -> Result<IUIAutomation, SendError> {
    ensure_com();
    unsafe {
        CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
            .map_err(|err| SendError::Ui(format!("UI Automation: {err}")))
    }
}

fn find_compose_hwnd(chat: HWND) -> Result<HWND, SendError> {
    if let Ok(edit) = uia_compose_element(chat) {
        let _ = unsafe { edit.SetFocus() };
        if let Ok(handle) = unsafe { edit.CurrentNativeWindowHandle() } {
            if !handle.is_invalid() && handle != chat {
                return Ok(handle);
            }
        }
    }
    find_richedit_child(chat).ok_or_else(|| SendError::Ui("no RichEdit compose HWND".into()))
}

fn uia_compose_element(hwnd: HWND) -> Result<IUIAutomationElement, SendError> {
    let auto = automation()?;
    let root = unsafe {
        auto.ElementFromHandle(hwnd)
            .map_err(|err| SendError::Ui(format!("ElementFromHandle: {err}")))?
    };
    find_compose_edit(&auto, &root)
}

fn paste_into_richedit(chat: HWND, edit: HWND, text: &str) -> Result<(), SendError> {
    set_clipboard_text(text)?;
    select_all_and_paste(edit);
    thread::sleep(Duration::from_millis(40));
    if compose_text(Some(edit)).trim() == text.trim() {
        return Ok(());
    }
    replace_sel(edit, text);
    thread::sleep(Duration::from_millis(40));
    if compose_text(Some(edit)).trim() == text.trim() {
        return Ok(());
    }
    // ValuePattern alone does not notify KakaoTalk; replace-sel after SetValue fires EN_CHANGE.
    let _ = uia_set_compose(chat, text);
    select_all_and_paste(edit);
    replace_sel(edit, text);
    thread::sleep(Duration::from_millis(40));
    if compose_text(Some(edit)).trim() == text.trim() {
        return Ok(());
    }
    Err(SendError::Ui(
        "could not put text in the KakaoTalk compose box".into(),
    ))
}

fn select_all_and_paste(edit: HWND) {
    unsafe {
        let _ = SendMessageW(edit, EM_SETSEL, WPARAM(0), LPARAM(-1));
        let _ = SendMessageW(edit, WM_PASTE, WPARAM(0), LPARAM(0));
        let _ = PostMessageW(edit, WM_PASTE, WPARAM(0), LPARAM(0));
    }
}

fn replace_sel(edit: HWND, text: &str) {
    let mut wide: Vec<u16> = text.encode_utf16().collect();
    wide.push(0);
    unsafe {
        let _ = SendMessageW(edit, EM_SETSEL, WPARAM(0), LPARAM(-1));
        let _ = SendMessageW(
            edit,
            EM_REPLACESEL,
            WPARAM(1),
            LPARAM(wide.as_ptr() as isize),
        );
    }
}

fn post_return_to_edit(edit: HWND) {
    unsafe {
        let key = WPARAM(VK_RETURN.0 as usize);
        let _ = SendMessageW(edit, WM_KEYDOWN, key, LPARAM(0));
        let _ = SendMessageW(edit, WM_CHAR, key, LPARAM(0));
        let _ = SendMessageW(edit, WM_KEYUP, key, LPARAM(1 << 30 | 1 << 31));
        let _ = PostMessageW(edit, WM_KEYDOWN, key, LPARAM(0));
        let _ = PostMessageW(edit, WM_CHAR, key, LPARAM(0));
        let _ = PostMessageW(edit, WM_KEYUP, key, LPARAM(0));
    }
}

fn compose_text(edit: Option<HWND>) -> String {
    let Some(edit) = edit.filter(|handle| !handle.is_invalid()) else {
        return String::new();
    };
    let via_message = window_text_message(edit);
    if !via_message.trim().is_empty() {
        return via_message;
    }
    window_title(edit)
}

fn window_text_message(hwnd: HWND) -> String {
    unsafe {
        let len = SendMessageW(hwnd, WM_GETTEXTLENGTH, WPARAM(0), LPARAM(0)).0;
        if len <= 0 {
            return String::new();
        }
        let mut buf = vec![0u16; len as usize + 1];
        let copied = SendMessageW(
            hwnd,
            WM_GETTEXT,
            WPARAM(buf.len()),
            LPARAM(buf.as_mut_ptr() as isize),
        )
        .0;
        if copied <= 0 {
            return String::new();
        }
        String::from_utf16_lossy(&buf[..copied as usize])
    }
}

fn find_richedit_child(parent: HWND) -> Option<HWND> {
    let mut found = HWND::default();
    unsafe {
        let _ = EnumChildWindows(
            parent,
            Some(enum_richedit_proc),
            LPARAM(&mut found as *mut HWND as isize),
        );
    }
    if found.is_invalid() {
        None
    } else {
        Some(found)
    }
}

unsafe extern "system" fn enum_richedit_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let found = &mut *(lparam.0 as *mut HWND);
    if !found.is_invalid() {
        return BOOL(0);
    }
    let mut buf = [0u16; 64];
    let len = GetClassNameW(hwnd, &mut buf);
    if len <= 0 {
        return BOOL(1);
    }
    let class = String::from_utf16_lossy(&buf[..len as usize]).to_ascii_lowercase();
    if class.contains("richedit") || class == "edit" {
        *found = hwnd;
        return BOOL(0);
    }
    BOOL(1)
}

fn uia_set_compose(hwnd: HWND, text: &str) -> Result<(), SendError> {
    let edit = uia_compose_element(hwnd)?;
    let unk = unsafe {
        edit.GetCurrentPattern(UIA_ValuePatternId)
            .map_err(|err| SendError::Ui(format!("Value pattern: {err}")))?
    };
    let value: IUIAutomationValuePattern = unk
        .cast()
        .map_err(|err| SendError::Ui(format!("Value cast: {err}")))?;
    unsafe {
        value
            .SetValue(&BSTR::from(text))
            .map_err(|err| SendError::Ui(format!("SetValue: {err}")))?;
    }
    Ok(())
}

fn find_compose_edit(
    auto: &IUIAutomation,
    root: &IUIAutomationElement,
) -> Result<IUIAutomationElement, SendError> {
    for control in [UIA_EditControlTypeId.0, UIA_DocumentControlTypeId.0] {
        if let Ok(found) = find_first_control(auto, root, control) {
            return Ok(found);
        }
    }
    let all = find_all(auto, root)?;
    let mut best: Option<(i32, IUIAutomationElement)> = None;
    for index in 0..all_len(&all) {
        let Ok(element) = (unsafe { all.GetElement(index) }) else {
            continue;
        };
        let Ok(unk) = (unsafe { element.GetCurrentPattern(UIA_ValuePatternId) }) else {
            continue;
        };
        let Ok(value): Result<IUIAutomationValuePattern, _> = unk.cast() else {
            continue;
        };
        let readonly = unsafe { value.CurrentIsReadOnly() }
            .map(|flag| is_true(flag))
            .unwrap_or(true);
        if readonly {
            continue;
        }
        let bottom = unsafe { element.CurrentBoundingRectangle() }
            .map(|rect| rect.bottom)
            .unwrap_or(0);
        match &best {
            Some((current, _)) if *current >= bottom => {}
            _ => best = Some((bottom, element)),
        }
    }
    best.map(|(_, element)| element)
        .ok_or_else(|| SendError::Ui("no compose box in KakaoTalk window".into()))
}

fn uia_invoke_send_if_enabled(hwnd: HWND) -> Result<(), SendError> {
    let auto = automation()?;
    let root = unsafe {
        auto.ElementFromHandle(hwnd)
            .map_err(|err| SendError::Ui(format!("ElementFromHandle: {err}")))?
    };
    if let Ok((element, invoke)) = find_named_send(&auto, &root, &["전송", "보내기", "Send"]) {
        if !uia_enabled(&element) {
            return Err(SendError::Ui("Send button disabled".into()));
        }
        unsafe {
            invoke
                .Invoke()
                .map_err(|err| SendError::Ui(format!("Invoke Send: {err}")))?;
        }
        return Ok(());
    }
    let buttons = find_all_control(&auto, &root, UIA_ButtonControlTypeId.0)?;
    let mut best: Option<(i32, IUIAutomationElement, IUIAutomationInvokePattern)> = None;
    for index in 0..all_len(&buttons) {
        let Ok(element) = (unsafe { buttons.GetElement(index) }) else {
            continue;
        };
        if !uia_enabled(&element) {
            continue;
        }
        let Ok(unk) = (unsafe { element.GetCurrentPattern(UIA_InvokePatternId) }) else {
            continue;
        };
        let Ok(invoke): Result<IUIAutomationInvokePattern, _> = unk.cast() else {
            continue;
        };
        let bottom = unsafe { element.CurrentBoundingRectangle() }
            .map(|rect| rect.bottom)
            .unwrap_or(0);
        match &best {
            Some((current, _, _)) if *current >= bottom => {}
            _ => best = Some((bottom, element, invoke)),
        }
    }
    let invoke = best
        .map(|(_, _, invoke)| invoke)
        .ok_or_else(|| SendError::Ui("no enabled Send button in KakaoTalk window".into()))?;
    unsafe {
        invoke
            .Invoke()
            .map_err(|err| SendError::Ui(format!("Invoke Send: {err}")))?;
    }
    Ok(())
}

fn uia_enabled(element: &IUIAutomationElement) -> bool {
    unsafe { element.CurrentIsEnabled() }
        .map(is_true)
        .unwrap_or(false)
}

fn find_named_send(
    auto: &IUIAutomation,
    root: &IUIAutomationElement,
    names: &[&str],
) -> Result<(IUIAutomationElement, IUIAutomationInvokePattern), SendError> {
    for name in names {
        let condition = unsafe {
            auto.CreatePropertyCondition(UIA_NamePropertyId, VARIANT::from(*name))
                .map_err(|err| SendError::Ui(err.to_string()))?
        };
        let Ok(element) = (unsafe { root.FindFirst(TreeScope_Descendants, &condition) }) else {
            continue;
        };
        let Ok(unk) = (unsafe { element.GetCurrentPattern(UIA_InvokePatternId) }) else {
            continue;
        };
        if let Ok(invoke) = unk.cast() {
            return Ok((element, invoke));
        }
    }
    Err(SendError::Ui("named Send control not found".into()))
}

fn uia_peek_bubbles(hwnd: HWND, compose: &str) -> Result<Vec<PeekBubble>, SendError> {
    let auto = automation()?;
    let root = unsafe {
        auto.ElementFromHandle(hwnd)
            .map_err(|err| SendError::Ui(format!("ElementFromHandle: {err}")))?
    };
    let all = find_all(&auto, &root)?;
    let mut window_rect = RECT::default();
    let _ = unsafe { GetWindowRect(hwnd, &mut window_rect) };
    let mid_x = (window_rect.left + window_rect.right) / 2;
    let compose_top = window_rect.bottom.saturating_sub(72);
    let mut scored = Vec::new();
    for index in 0..all_len(&all) {
        let Ok(element) = (unsafe { all.GetElement(index) }) else {
            continue;
        };
        if is_compose_element(&element) {
            continue;
        }
        let rect = unsafe { element.CurrentBoundingRectangle() }.unwrap_or_default();
        if rect.bottom <= rect.top || rect.top >= compose_top {
            continue;
        }
        let Some(text) = element_visible_text(&element) else {
            continue;
        };
        if !keep_bubble_text(&text) {
            continue;
        }
        if !compose.is_empty() && text.trim() == compose {
            continue;
        }
        let direction = if rect.left >= mid_x {
            "outgoing"
        } else {
            "incoming"
        };
        scored.push((rect.top, rect.left, PeekBubble { direction, text }));
    }
    scored.sort_by_key(|(top, left, _)| (*top, *left));
    scored.dedup_by(|a, b| a.2.text == b.2.text && a.2.direction == b.2.direction);
    const KEEP: usize = 20;
    let start = scored.len().saturating_sub(KEEP);
    Ok(scored
        .into_iter()
        .skip(start)
        .map(|(_, _, bubble)| bubble)
        .collect())
}

fn is_compose_element(element: &IUIAutomationElement) -> bool {
    if let Ok(control) = unsafe { element.CurrentControlType() } {
        if control == UIA_EditControlTypeId || control == UIA_DocumentControlTypeId {
            return true;
        }
    }
    if let Ok(class) = unsafe { element.CurrentClassName() } {
        let class = class.to_string().to_ascii_lowercase();
        if class.contains("richedit") {
            return true;
        }
    }
    if let Ok(unk) = unsafe { element.GetCurrentPattern(UIA_ValuePatternId) } {
        if let Ok(value) = unk.cast::<IUIAutomationValuePattern>() {
            let readonly = unsafe { value.CurrentIsReadOnly() }
                .map(is_true)
                .unwrap_or(true);
            if !readonly {
                return true;
            }
        }
    }
    false
}

fn element_visible_text(element: &IUIAutomationElement) -> Option<String> {
    let candidates = [
        unsafe { element.CurrentName() }
            .ok()
            .map(|name| name.to_string()),
        property_string(element, UIA_LegacyIAccessibleNamePropertyId),
        text_pattern_text(element),
        readonly_value(element),
    ];
    candidates.into_iter().flatten().find_map(|text| {
        let text = text.trim().to_string();
        if keep_bubble_text(&text) {
            Some(text)
        } else {
            None
        }
    })
}

fn property_string(element: &IUIAutomationElement, property: UIA_PROPERTY_ID) -> Option<String> {
    let variant = unsafe { element.GetCurrentPropertyValue(property) }.ok()?;
    BSTR::try_from(&variant).ok().map(|value| value.to_string())
}

fn text_pattern_text(element: &IUIAutomationElement) -> Option<String> {
    let unk = unsafe { element.GetCurrentPattern(UIA_TextPatternId) }.ok()?;
    let pattern: IUIAutomationTextPattern = unk.cast().ok()?;
    let range = unsafe { pattern.DocumentRange() }.ok()?;
    unsafe { range.GetText(-1) }
        .ok()
        .map(|text| text.to_string())
}

fn readonly_value(element: &IUIAutomationElement) -> Option<String> {
    let unk = unsafe { element.GetCurrentPattern(UIA_ValuePatternId) }.ok()?;
    let value: IUIAutomationValuePattern = unk.cast().ok()?;
    let readonly = unsafe { value.CurrentIsReadOnly() }
        .map(is_true)
        .unwrap_or(false);
    if !readonly {
        return None;
    }
    unsafe { value.CurrentValue() }
        .ok()
        .map(|text| text.to_string())
}

fn find_first_control(
    auto: &IUIAutomation,
    root: &IUIAutomationElement,
    control: i32,
) -> Result<IUIAutomationElement, SendError> {
    let condition = unsafe {
        auto.CreatePropertyCondition(UIA_ControlTypePropertyId, VARIANT::from(control))
            .map_err(|err| SendError::Ui(err.to_string()))?
    };
    unsafe {
        root.FindFirst(TreeScope_Descendants, &condition)
            .map_err(|err| SendError::Ui(err.to_string()))
    }
}

fn find_all_control(
    auto: &IUIAutomation,
    root: &IUIAutomationElement,
    control: i32,
) -> Result<IUIAutomationElementArray, SendError> {
    let condition = unsafe {
        auto.CreatePropertyCondition(UIA_ControlTypePropertyId, VARIANT::from(control))
            .map_err(|err| SendError::Ui(err.to_string()))?
    };
    unsafe {
        root.FindAll(TreeScope_Descendants, &condition)
            .map_err(|err| SendError::Ui(err.to_string()))
    }
}

fn find_all(
    auto: &IUIAutomation,
    root: &IUIAutomationElement,
) -> Result<IUIAutomationElementArray, SendError> {
    let condition = unsafe {
        auto.CreateTrueCondition()
            .map_err(|err| SendError::Ui(err.to_string()))?
    };
    unsafe {
        root.FindAll(TreeScope_Descendants, &condition)
            .map_err(|err| SendError::Ui(err.to_string()))
    }
}

fn all_len(array: &IUIAutomationElementArray) -> i32 {
    unsafe { array.Length() }.unwrap_or(0)
}
