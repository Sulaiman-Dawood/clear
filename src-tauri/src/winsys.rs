//! Windows-specific plumbing: reading the selection, pasting results back,
//! window focus, caret position and non-activating popups.
//!
//! The selection is read the same way a user would: copy it, read the
//! clipboard, then put the user's clipboard back the way it was.

use std::thread::sleep;
use std::time::{Duration, Instant};

/// What the clipboard held before we borrowed it: every format, as raw
/// bytes, so text, formatting (HTML/RTF), images and copied files all come
/// back exactly as they were.
#[cfg(windows)]
pub struct SavedClipboard {
    formats: Vec<(u32, Vec<u8>)>,
}

#[cfg(windows)]
pub fn snapshot_clipboard() -> SavedClipboard {
    SavedClipboard { formats: clip::read_all() }
}

#[cfg(windows)]
pub fn restore_clipboard(saved: SavedClipboard) {
    clip::write_all(&saved.formats);
}

#[cfg(not(windows))]
pub enum SavedClipboard {
    Text(String),
    Empty,
}

#[cfg(not(windows))]
pub fn snapshot_clipboard() -> SavedClipboard {
    arboard::Clipboard::new()
        .and_then(|mut cb| cb.get_text())
        .map(SavedClipboard::Text)
        .unwrap_or(SavedClipboard::Empty)
}

#[cfg(not(windows))]
pub fn restore_clipboard(saved: SavedClipboard) {
    let Ok(mut cb) = arboard::Clipboard::new() else { return };
    let _ = match saved {
        SavedClipboard::Text(t) => cb.set_text(t),
        SavedClipboard::Empty => cb.clear(),
    };
}

/// Raw Windows clipboard access for saving and restoring every format.
#[cfg(windows)]
mod clip {
    use super::*;
    use windows::core::w;
    use windows::Win32::Foundation::{HANDLE, HGLOBAL};
    use windows::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData, OpenClipboard,
        RegisterClipboardFormatW, SetClipboardData,
    };
    use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE};

    /// Formats backed by GDI handles rather than memory; Windows recreates
    /// them from the memory formats (e.g. a bitmap from CF_DIB).
    const HANDLE_FORMATS: &[u32] = &[2, 3, 9, 14, 0x80, 0x82, 0x83, 0x8E];
    /// Don't hold more than this in memory (e.g. a huge copied image).
    const MAX_TOTAL: usize = 64 * 1024 * 1024;

    fn open() -> bool {
        for _ in 0..20 {
            if unsafe { OpenClipboard(None) }.is_ok() {
                return true;
            }
            sleep(Duration::from_millis(10));
        }
        false
    }

    pub fn read_all() -> Vec<(u32, Vec<u8>)> {
        let mut out = Vec::new();
        if !open() {
            return out;
        }
        let mut total = 0usize;
        unsafe {
            let mut format = EnumClipboardFormats(0);
            while format != 0 {
                if !HANDLE_FORMATS.contains(&format) {
                    if let Ok(handle) = GetClipboardData(format) {
                        let mem = HGLOBAL(handle.0);
                        let size = GlobalSize(mem);
                        let ptr = GlobalLock(mem) as *const u8;
                        if !ptr.is_null() {
                            if size > 0 && total + size <= MAX_TOTAL {
                                out.push((format, std::slice::from_raw_parts(ptr, size).to_vec()));
                                total += size;
                            }
                            let _ = GlobalUnlock(mem);
                        }
                    }
                }
                format = EnumClipboardFormats(format);
            }
            let _ = CloseClipboard();
        }
        out
    }

    pub fn write_all(formats: &[(u32, Vec<u8>)]) {
        if !open() {
            return;
        }
        unsafe {
            let _ = EmptyClipboard();
            for (format, bytes) in formats {
                put(*format, bytes);
            }
            if !formats.is_empty() {
                // Restoring isn't a new copy: keep it out of Win+V history.
                let no_history = RegisterClipboardFormatW(w!("CanIncludeInClipboardHistory"));
                put(no_history, &0u32.to_le_bytes());
            }
            let _ = CloseClipboard();
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// Uses the real clipboard, so it only runs on request:
        /// `cargo test -- --ignored clipboard`. Your clipboard is put back after.
        #[test]
        #[ignore]
        fn clipboard_round_trip_keeps_every_format() {
            let users_clipboard = read_all();

            let html_format = unsafe { RegisterClipboardFormatW(w!("HTML Format")) };
            let text: Vec<u8> = "Clear test ✓\0".encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
            let html = b"Version:0.9\r\n<b>Clear test</b>\0".to_vec();
            write_all(&[(13, text.clone()), (html_format, html.clone())]); // 13 = CF_UNICODETEXT

            let saved = read_all();
            write_all(&[]); // what copying a selection would do in between
            write_all(&saved);
            let back = read_all();

            write_all(&users_clipboard);

            let get = |f: u32| back.iter().find(|(x, _)| *x == f).map(|(_, b)| b.clone());
            assert!(get(13).is_some_and(|b| b.starts_with(&text)), "plain text lost");
            assert!(get(html_format).is_some_and(|b| b.starts_with(&html)), "formatted (HTML) text lost");
        }
    }

    unsafe fn put(format: u32, bytes: &[u8]) {
        let Ok(mem) = GlobalAlloc(GMEM_MOVEABLE, bytes.len()) else { return };
        let ptr = GlobalLock(mem) as *mut u8;
        if ptr.is_null() {
            return;
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
        let _ = GlobalUnlock(mem);
        // On success the clipboard owns the memory.
        let _ = SetClipboardData(format, Some(HANDLE(mem.0)));
    }
}

/// Put text on the clipboard without adding it to Windows clipboard history.
fn set_text_private(cb: &mut arboard::Clipboard, text: &str) -> Result<(), arboard::Error> {
    #[cfg(windows)]
    {
        use arboard::SetExtWindows;
        cb.set().exclude_from_history().text(text.to_string())
    }
    #[cfg(not(windows))]
    {
        cb.set_text(text.to_string())
    }
}

/// Set the clipboard for the user to keep (the Copy button).
pub fn set_clipboard_text(text: &str) -> Result<(), String> {
    let mut cb = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    cb.set_text(normalize_newlines(text)).map_err(|e| e.to_string())
}

/// Windows controls expect CRLF line endings on the clipboard.
pub fn normalize_newlines(text: &str) -> String {
    if cfg!(windows) {
        text.replace("\r\n", "\n").replace('\n', "\r\n")
    } else {
        text.replace("\r\n", "\n")
    }
}

#[cfg(windows)]
mod imp {
    use super::*;
    use windows::core::PWSTR;
    use windows::Win32::Foundation::{CloseHandle, HWND, POINT, RECT};
    use windows::Win32::Graphics::Gdi::{
        ClientToScreen, GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    use windows::Win32::System::DataExchange::GetClipboardSequenceNumber;
    use windows::Win32::System::Threading::{
        AttachThreadInput, GetCurrentProcessId, GetCurrentThreadId, OpenProcess, QueryFullProcessImageNameW,
        PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
        KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_C, VK_CONTROL, VK_INSERT, VK_LMENU, VK_LSHIFT, VK_LWIN,
        VK_MENU, VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SHIFT, VK_V,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        BringWindowToTop, GetCursorPos, GetForegroundWindow, GetGUIThreadInfo, GetWindowLongPtrW,
        GetWindowThreadProcessId, IsIconic, IsWindow, SetForegroundWindow, SetWindowLongPtrW,
        SetWindowPos, ShowWindow, GUITHREADINFO, GWL_EXSTYLE, HWND_TOPMOST, SWP_NOACTIVATE,
        SWP_NOMOVE, SWP_NOSIZE, SW_RESTORE, SW_SHOWNOACTIVATE, WS_EX_APPWINDOW, WS_EX_NOACTIVATE,
        WS_EX_TOOLWINDOW,
    };

    fn hwnd(raw: isize) -> HWND {
        HWND(raw as *mut core::ffi::c_void)
    }

    pub fn foreground_window() -> isize {
        unsafe { GetForegroundWindow().0 as isize }
    }

    pub fn is_own_window(raw: isize) -> bool {
        if raw == 0 {
            return false;
        }
        let mut pid = 0u32;
        unsafe {
            GetWindowThreadProcessId(hwnd(raw), Some(&mut pid));
            pid == GetCurrentProcessId()
        }
    }

    /// Lower-case executable name of the process that owns a window.
    pub fn window_exe(raw: isize) -> Option<String> {
        let mut pid = 0u32;
        unsafe {
            GetWindowThreadProcessId(hwnd(raw), Some(&mut pid));
            if pid == 0 {
                return None;
            }
            let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
            let mut buf = [0u16; 1024];
            let mut len = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len);
            let _ = CloseHandle(process);
            ok.ok()?;
            let path = String::from_utf16_lossy(&buf[..len as usize]);
            path.rsplit(['\\', '/']).next().map(|s| s.to_lowercase())
        }
    }

    /// In terminals Ctrl+C with nothing selected interrupts the running
    /// program, so we copy with Ctrl+Insert there instead.
    fn is_terminal(raw: isize) -> bool {
        const TERMINALS: &[&str] = &[
            "windowsterminal.exe",
            "wt.exe",
            "openconsole.exe",
            "conhost.exe",
            "cmd.exe",
            "powershell.exe",
            "pwsh.exe",
            "wezterm-gui.exe",
            "alacritty.exe",
            "mintty.exe",
            "tabby.exe",
            "hyper.exe",
            "putty.exe",
            "kitty.exe",
        ];
        window_exe(raw).is_some_and(|exe| TERMINALS.contains(&exe.as_str()))
    }

    fn key_down(vk: VIRTUAL_KEY) -> bool {
        unsafe { (GetAsyncKeyState(vk.0 as i32) as u16 & 0x8000) != 0 }
    }

    /// Wait until the user lets go of Shift/Alt/Win/Ctrl from the hotkey,
    /// so our synthetic Ctrl+C isn't seen as Ctrl+Shift+C.
    fn wait_for_modifiers_released(timeout: Duration) {
        let start = Instant::now();
        let mods = [VK_SHIFT, VK_MENU, VK_LWIN, VK_RWIN, VK_CONTROL];
        while start.elapsed() < timeout {
            if !mods.iter().any(|&m| key_down(m)) {
                return;
            }
            sleep(Duration::from_millis(10));
        }
    }

    fn key_input(vk: VIRTUAL_KEY, up: bool) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk,
                    wScan: 0,
                    dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) },
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }

    /// Send Ctrl+<key>, releasing any modifiers the user is still holding.
    fn send_ctrl_combo(key: VIRTUAL_KEY) {
        let mut inputs = Vec::new();
        for m in [VK_LSHIFT, VK_RSHIFT, VK_LMENU, VK_RMENU, VK_LWIN, VK_RWIN] {
            if key_down(m) {
                inputs.push(key_input(m, true));
            }
        }
        inputs.push(key_input(VK_CONTROL, false));
        inputs.push(key_input(key, false));
        inputs.push(key_input(key, true));
        inputs.push(key_input(VK_CONTROL, true));
        unsafe {
            SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
        }
    }

    fn clipboard_seq() -> u32 {
        unsafe { GetClipboardSequenceNumber() }
    }

    /// Copy the selection in the given window and return it.
    /// The user's clipboard is restored afterwards.
    /// Returns `None` when nothing is selected.
    pub fn capture_selection(target: isize) -> Option<String> {
        let saved = snapshot_clipboard();
        let before = clipboard_seq();
        // Most people let go of the shortcut within ~100 ms. Don't wait longer:
        // send_ctrl_combo releases any modifier that's still held.
        wait_for_modifiers_released(Duration::from_millis(150));
        let copy_key = if is_terminal(target) { VK_INSERT } else { VK_C };
        send_ctrl_combo(copy_key);

        // Apps answer a copy within a few dozen ms; when nothing is selected
        // the clipboard never changes, so this timeout is the "no selection" cost.
        let start = Instant::now();
        let mut changed = false;
        while start.elapsed() < Duration::from_millis(350) {
            if clipboard_seq() != before {
                changed = true;
                break;
            }
            sleep(Duration::from_millis(4));
        }

        let mut text = None;
        if changed {
            // Give the source app a moment to finish writing all formats.
            sleep(Duration::from_millis(15));
            for _ in 0..5 {
                if let Ok(mut cb) = arboard::Clipboard::new() {
                    if let Ok(t) = cb.get_text() {
                        text = Some(t);
                        break;
                    }
                }
                sleep(Duration::from_millis(20));
            }
            restore_clipboard(saved);
        }
        text.filter(|t| !t.trim().is_empty())
    }

    /// Bring a window back to the front. Returns true once it's foreground.
    pub fn focus_window(raw: isize) -> bool {
        let h = hwnd(raw);
        unsafe {
            if raw == 0 || !IsWindow(Some(h)).as_bool() {
                return false;
            }
            if IsIconic(h).as_bool() {
                let _ = ShowWindow(h, SW_RESTORE);
            }
            if GetForegroundWindow() == h {
                return true;
            }
            let current = GetCurrentThreadId();
            let fg_thread = GetWindowThreadProcessId(GetForegroundWindow(), None);
            let attached = fg_thread != 0
                && fg_thread != current
                && AttachThreadInput(current, fg_thread, true).as_bool();
            let _ = SetForegroundWindow(h);
            let _ = BringWindowToTop(h);
            if attached {
                let _ = AttachThreadInput(current, fg_thread, false);
            }
            let start = Instant::now();
            while start.elapsed() < Duration::from_millis(400) {
                if GetForegroundWindow() == h {
                    return true;
                }
                sleep(Duration::from_millis(10));
            }
            false
        }
    }

    /// Paste text into the given window, then restore the user's clipboard.
    pub fn paste_into(target: isize, text: &str) -> Result<(), String> {
        let saved = snapshot_clipboard();
        if !focus_window(target) {
            return Err("Couldn't return to the original window to paste. The result is still in Clear.".into());
        }
        {
            let mut cb = arboard::Clipboard::new().map_err(|e| e.to_string())?;
            set_text_private(&mut cb, &normalize_newlines(text)).map_err(|e| e.to_string())?;
        }
        wait_for_modifiers_released(Duration::from_millis(800));
        sleep(Duration::from_millis(40));
        send_ctrl_combo(VK_V);
        // Apps read the clipboard asynchronously; wait before restoring it.
        sleep(Duration::from_millis(350));
        restore_clipboard(saved);
        Ok(())
    }

    /// Screen rectangle (left, top, right, bottom) of the text caret in the
    /// window's thread, when the app exposes one (classic Win32 controls do;
    /// browsers and Electron apps usually don't).
    pub fn caret_rect(target: isize) -> Option<(i32, i32, i32, i32)> {
        unsafe {
            let tid = GetWindowThreadProcessId(hwnd(target), None);
            if tid == 0 {
                return None;
            }
            let mut info = GUITHREADINFO {
                cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
                ..Default::default()
            };
            GetGUIThreadInfo(tid, &mut info).ok()?;
            if info.hwndCaret.is_invalid() {
                return None;
            }
            let rc = info.rcCaret;
            let mut tl = POINT { x: rc.left, y: rc.top };
            let mut br = POINT { x: rc.right, y: rc.bottom };
            if !ClientToScreen(info.hwndCaret, &mut tl).as_bool() || !ClientToScreen(info.hwndCaret, &mut br).as_bool() {
                return None;
            }
            if br.y <= tl.y {
                return None;
            }
            Some((tl.x, tl.y, br.x.max(tl.x + 1), br.y))
        }
    }

    pub fn cursor_pos() -> (i32, i32) {
        let mut p = POINT::default();
        unsafe {
            let _ = GetCursorPos(&mut p);
        }
        (p.x, p.y)
    }

    /// Usable area (excluding the taskbar) of the monitor containing a point.
    pub fn work_area_at(x: i32, y: i32) -> (i32, i32, i32, i32) {
        unsafe {
            let mon = MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST);
            let mut mi = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            if GetMonitorInfoW(mon, &mut mi).as_bool() {
                let RECT { left, top, right, bottom } = mi.rcWork;
                (left, top, right, bottom)
            } else {
                (0, 0, 1920, 1080)
            }
        }
    }

    /// Make a window never take focus when shown or clicked (used for the
    /// progress toast so the user's app keeps the keyboard).
    pub fn set_no_activate(raw: isize, on: bool) {
        unsafe {
            let h = hwnd(raw);
            let ex = GetWindowLongPtrW(h, GWL_EXSTYLE);
            let flag = WS_EX_NOACTIVATE.0 as isize;
            let new = if on { ex | flag } else { ex & !flag };
            if new != ex {
                SetWindowLongPtrW(h, GWL_EXSTYLE, new);
            }
        }
    }

    /// Keep a window out of Alt+Tab and the taskbar.
    pub fn make_tool_window(raw: isize) {
        unsafe {
            let h = hwnd(raw);
            let ex = GetWindowLongPtrW(h, GWL_EXSTYLE);
            let new = (ex | WS_EX_TOOLWINDOW.0 as isize) & !(WS_EX_APPWINDOW.0 as isize);
            if new != ex {
                SetWindowLongPtrW(h, GWL_EXSTYLE, new);
            }
        }
    }

    pub fn show_no_activate(raw: isize) {
        unsafe {
            let h = hwnd(raw);
            let _ = ShowWindow(h, SW_SHOWNOACTIVATE);
            let _ = SetWindowPos(h, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        }
    }
}

#[cfg(windows)]
pub use imp::*;

/// Move and resize a window in one step (physical pixels), so it never
/// shows an in-between state.
#[cfg(windows)]
pub fn set_bounds(raw: isize, x: i32, y: i32, w: i32, h: i32) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{SetWindowPos, SWP_NOACTIVATE, SWP_NOOWNERZORDER, SWP_NOZORDER};
    if raw != 0 {
        unsafe {
            let _ = SetWindowPos(
                HWND(raw as *mut core::ffi::c_void),
                None,
                x,
                y,
                w,
                h,
                SWP_NOZORDER | SWP_NOOWNERZORDER | SWP_NOACTIVATE,
            );
        }
    }
}

#[cfg(windows)]
pub fn hide_window(raw: isize) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE};
    if raw != 0 {
        unsafe {
            let _ = ShowWindow(HWND(raw as *mut core::ffi::c_void), SW_HIDE);
        }
    }
}

/// Is the keyboard focus in something you can type into (a text box, an
/// editor), as opposed to read-only text like a web page or PDF?
///
/// Uses UI Automation, which most apps support. Start it, do other work
/// (like copying the selection), then `wait()` for the answer. When an app
/// doesn't answer in time, assume it's editable (pasting is the old behavior).
#[cfg(windows)]
pub struct EditableCheck {
    target: isize,
    started: Instant,
    rx: std::sync::mpsc::Receiver<Option<bool>>,
}

#[cfg(windows)]
impl EditableCheck {
    pub fn start(target: isize) -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(uia_focus_editable());
        });
        Self { target, started: Instant::now(), rx }
    }

    pub fn wait(self) -> bool {
        let left = Duration::from_millis(700).saturating_sub(self.started.elapsed());
        match self.rx.recv_timeout(left) {
            Ok(Some(editable)) => editable,
            // No UI Automation answer: fall back to the Win32 text caret.
            Ok(None) => caret_rect(self.target).is_some(),
            Err(_) => true,
        }
    }
}

#[cfg(windows)]
fn uia_focus_editable() -> Option<bool> {
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
    };
    use windows::Win32::UI::Accessibility::{
        CUIAutomation, IUIAutomation, IUIAutomationValuePattern, UIA_ComboBoxControlTypeId,
        UIA_EditControlTypeId, UIA_ValuePatternId,
    };
    unsafe {
        let initialized = CoInitializeEx(None, COINIT_MULTITHREADED).is_ok();
        let result = (|| -> windows::core::Result<Option<bool>> {
            let uia: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)?;
            let el = uia.GetFocusedElement()?;
            // Text fields (including rich editors in browsers) report whether
            // they are read-only through the Value pattern.
            if let Ok(value) = el.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId) {
                return Ok(Some(!value.CurrentIsReadOnly()?.as_bool()));
            }
            let kind = el.CurrentControlType()?;
            if kind == UIA_EditControlTypeId || kind == UIA_ComboBoxControlTypeId {
                return Ok(Some(true));
            }
            // A document without a Value pattern could be an editor (Word) or a
            // page you're reading (a browser); something else (a pane, page
            // text) is unknown too. The caller decides from the text caret.
            Ok(None)
        })();
        if initialized {
            CoUninitialize();
        }
        result.ok().flatten()
    }
}

// Stubs so the crate still compiles on other platforms; Clear targets Windows first.
#[cfg(not(windows))]
mod imp_other {
    pub fn foreground_window() -> isize { 0 }
    pub fn is_own_window(_: isize) -> bool { false }
    pub fn window_exe(_: isize) -> Option<String> { None }
    pub fn capture_selection(_: isize) -> Option<String> { None }
    pub fn focus_window(_: isize) -> bool { false }
    pub fn paste_into(_: isize, _: &str) -> Result<(), String> {
        Err("Pasting into other apps is only supported on Windows for now.".into())
    }
    pub fn caret_rect(_: isize) -> Option<(i32, i32, i32, i32)> { None }
    pub fn cursor_pos() -> (i32, i32) { (200, 200) }
    pub fn work_area_at(_: i32, _: i32) -> (i32, i32, i32, i32) { (0, 0, 1440, 900) }
    pub fn set_no_activate(_: isize, _: bool) {}
    pub fn make_tool_window(_: isize) {}
    pub fn hide_window(_: isize) {}
    pub fn set_bounds(_: isize, _: i32, _: i32, _: i32, _: i32) {}
    pub struct EditableCheck;
    impl EditableCheck {
        pub fn start(_: isize) -> Self { EditableCheck }
        pub fn wait(self) -> bool { true }
    }
    pub fn show_no_activate(_: isize) {}
}

#[cfg(not(windows))]
pub use imp_other::*;
