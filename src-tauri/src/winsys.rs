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
    use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
    use windows::Win32::Graphics::Gdi::{
        ClientToScreen, GetMonitorInfoW, MonitorFromPoint, PatBlt, BLACKNESS, HDC, MONITORINFO,
        MONITOR_DEFAULTTONEAREST,
    };
    use windows::Win32::System::DataExchange::GetClipboardSequenceNumber;
    use windows::Win32::System::Threading::{
        AttachThreadInput, GetCurrentProcessId, GetCurrentThreadId, OpenProcess, QueryFullProcessImageNameW,
        PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, GetLastInputInfo, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT,
        KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, LASTINPUTINFO, VIRTUAL_KEY, VK_A, VK_C, VK_CONTROL,
        VK_INSERT, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_MENU, VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SHIFT,
        VK_V,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        BringWindowToTop, CallWindowProcW, GetClientRect, GetCursorPos, GetForegroundWindow,
        GetGUIThreadInfo, GetWindowLongPtrW, GetWindowThreadProcessId, IsIconic, IsWindow,
        SetForegroundWindow, SetWindowLongPtrW, SetWindowPos, ShowWindow, GUITHREADINFO,
        GWLP_WNDPROC, GWL_EXSTYLE, GWL_STYLE, HWND_TOPMOST, SWP_FRAMECHANGED, SWP_NOACTIVATE,
        SWP_NOMOVE, SWP_NOSIZE, SW_RESTORE, SW_SHOW, SW_SHOWNOACTIVATE, WM_ERASEBKGND,
        WM_NCACTIVATE, WM_NCPAINT, WNDPROC, WS_BORDER, WS_CAPTION, WS_CLIPCHILDREN,
        WS_CLIPSIBLINGS, WS_DLGFRAME, WS_EX_APPWINDOW, WS_EX_CLIENTEDGE, WS_EX_DLGMODALFRAME,
        WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_WINDOWEDGE, WS_MAXIMIZEBOX,
        WS_MINIMIZEBOX, WS_POPUP, WS_SYSMENU, WS_THICKFRAME,
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
    pub fn is_terminal_window(raw: isize) -> bool {
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

    pub fn last_input_tick() -> u32 {
        let mut info = LASTINPUTINFO {
            cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
            dwTime: 0,
        };
        unsafe {
            if GetLastInputInfo(&mut info).as_bool() {
                info.dwTime
            } else {
                0
            }
        }
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
        capture_selection_with_timeout(target, Duration::from_millis(350))
    }

    /// Re-copy the selection to verify the user didn't move away; allows a
    /// slightly longer timeout since text is expected to be selected.
    pub fn verify_selection(target: isize) -> Option<String> {
        capture_selection_with_timeout(target, Duration::from_millis(550))
    }

    /// Select all text in the active editable input field (Ctrl+A) and copy it
    /// (Ctrl+C), restoring the user's clipboard afterwards. Leaves all text in
    /// the field selected so an in-place or Replace paste overwrites it cleanly.
    /// Returns `None` when the field is empty.
    pub fn capture_all_in_field(_target: isize) -> Option<String> {
        let saved = snapshot_clipboard();
        let before = clipboard_seq();
        wait_for_modifiers_released(Duration::from_millis(150));
        send_ctrl_combo(VK_A);
        sleep(Duration::from_millis(35));
        send_ctrl_combo(VK_C);

        let start = Instant::now();
        let mut changed = false;
        while start.elapsed() < Duration::from_millis(280) {
            if clipboard_seq() != before {
                changed = true;
                break;
            }
            sleep(Duration::from_millis(4));
        }

        let mut text = None;
        if changed {
            sleep(Duration::from_millis(15));
            for _ in 0..10 {
                if let Ok(mut cb) = arboard::Clipboard::new() {
                    if let Ok(t) = cb.get_text() {
                        text = Some(t);
                        break;
                    }
                }
                sleep(Duration::from_millis(15));
            }
            restore_clipboard(saved);
        }
        text.filter(|t| !t.trim().is_empty())
    }

    fn capture_selection_with_timeout(target: isize, timeout: Duration) -> Option<String> {
        let saved = snapshot_clipboard();
        let before = clipboard_seq();
        // Most people let go of the shortcut within ~100 ms. Don't wait longer:
        // send_ctrl_combo releases any modifier that's still held.
        wait_for_modifiers_released(Duration::from_millis(150));
        let copy_key = if is_terminal_window(target) { VK_INSERT } else { VK_C };
        send_ctrl_combo(copy_key);

        // Apps answer a copy within a few dozen ms; when nothing is selected
        // the clipboard never changes, so this timeout is the "no selection" cost.
        let start = Instant::now();
        let mut changed = false;
        while start.elapsed() < timeout {
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
            for _ in 0..10 {
                if let Ok(mut cb) = arboard::Clipboard::new() {
                    if let Ok(t) = cb.get_text() {
                        text = Some(t);
                        break;
                    }
                }
                sleep(Duration::from_millis(15));
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

    /// DPI scale factor of the monitor containing a point (e.g. 1.0, 1.25, 1.5).
    pub fn monitor_scale_at(x: i32, y: i32) -> Option<f64> {
        unsafe {
            let mon = MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST);
            extern "system" {
                fn LoadLibraryW(lpLibFileName: *const u16) -> isize;
                fn GetProcAddress(hModule: isize, lpProcName: *const u8) -> *const core::ffi::c_void;
            }
            let dll: Vec<u16> = "shcore.dll\0".encode_utf16().collect();
            let shcore = LoadLibraryW(dll.as_ptr());
            if shcore == 0 {
                return None;
            }
            let proc = GetProcAddress(shcore, b"GetDpiForMonitor\0".as_ptr());
            if proc.is_null() {
                return None;
            }
            type FnGetDpiForMonitor = unsafe extern "system" fn(
                hmonitor: *mut core::ffi::c_void,
                dpi_type: i32,
                dpi_x: *mut u32,
                dpi_y: *mut u32,
            ) -> i32;
            let get_dpi: FnGetDpiForMonitor = std::mem::transmute(proc);
            let mut dpi_x = 0u32;
            let mut dpi_y = 0u32;
            // MDT_EFFECTIVE_DPI = 0
            if get_dpi(mon.0, 0, &mut dpi_x, &mut dpi_y) == 0 && dpi_x > 0 {
                Some(dpi_x as f64 / 96.0)
            } else {
                None
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

    static POPUP_ORIG_WNDPROC: std::sync::atomic::AtomicIsize =
        std::sync::atomic::AtomicIsize::new(0);

    unsafe extern "system" fn popup_wndproc(
        h: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match msg {
            // Never let DefWindowProc paint a classic Win32 title bar or border.
            WM_NCPAINT => return LRESULT(0),
            // Clear the GDI surface to transparent black (0x00000000) so no
            // stale pixels show through the transparent margin around the card.
            WM_ERASEBKGND => {
                let hdc = HDC(wparam.0 as *mut core::ffi::c_void);
                let mut rc = RECT::default();
                if GetClientRect(h, &mut rc).is_ok() {
                    let _ = PatBlt(hdc, 0, 0, rc.right - rc.left, rc.bottom - rc.top, BLACKNESS);
                }
                return LRESULT(1);
            }
            _ => {}
        }
        let orig = POPUP_ORIG_WNDPROC.load(std::sync::atomic::Ordering::SeqCst);
        let prev: WNDPROC = if orig != 0 {
            Some(std::mem::transmute::<
                isize,
                unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT,
            >(orig))
        } else {
            None
        };
        // Passing lParam = -1 on WM_NCACTIVATE tells DefWindowProc not to
        // repaint the non-client title bar while still letting Tao track
        // activation / focus loss.
        let lp = if msg == WM_NCACTIVATE { LPARAM(-1) } else { lparam };
        CallWindowProcW(prev, h, msg, wparam, lp)
    }

    /// Configure the popup as a pure frameless WS_POPUP tool window with no
    /// caption, no classic border, and no Windows 11 DWM drop shadow or corner
    /// rounding. Calling this once while hidden (and showing/hiding via raw
    /// Win32 ShowWindow instead of Tao's set_visible) keeps Tao from
    /// re-applying WS_CAPTION and flashing an old window frame.
    pub fn configure_popup_window(raw: isize) {
        if raw == 0 {
            return;
        }
        unsafe {
            let h = hwnd(raw);

            let ours = popup_wndproc as *const () as isize;
            let cur = GetWindowLongPtrW(h, GWLP_WNDPROC);
            if cur != 0 && cur != ours {
                POPUP_ORIG_WNDPROC.store(cur, std::sync::atomic::Ordering::SeqCst);
                let prev = SetWindowLongPtrW(h, GWLP_WNDPROC, ours);
                if prev != 0 && prev != ours {
                    POPUP_ORIG_WNDPROC.store(prev, std::sync::atomic::Ordering::SeqCst);
                }
            }

            let mut changed = false;

            let style = GetWindowLongPtrW(h, GWL_STYLE);
            let strip_style = (WS_CAPTION.0
                | WS_THICKFRAME.0
                | WS_SYSMENU.0
                | WS_BORDER.0
                | WS_DLGFRAME.0
                | WS_MINIMIZEBOX.0
                | WS_MAXIMIZEBOX.0) as isize;
            let add_style = (WS_POPUP.0 | WS_CLIPSIBLINGS.0 | WS_CLIPCHILDREN.0) as isize;
            let new_style = (style & !strip_style) | add_style;
            if new_style != style {
                SetWindowLongPtrW(h, GWL_STYLE, new_style);
                changed = true;
            }

            let ex = GetWindowLongPtrW(h, GWL_EXSTYLE);
            let strip_ex = (WS_EX_WINDOWEDGE.0
                | WS_EX_DLGMODALFRAME.0
                | WS_EX_CLIENTEDGE.0
                | WS_EX_APPWINDOW.0) as isize;
            let add_ex = (WS_EX_TOOLWINDOW.0 | WS_EX_TOPMOST.0) as isize;
            let new_ex = (ex & !strip_ex) | add_ex;
            if new_ex != ex {
                SetWindowLongPtrW(h, GWL_EXSTYLE, new_ex);
                changed = true;
            }

            if !changed {
                return;
            }

            // Prevent Windows 11 from rounding or clipping the transparent HWND corners.
            // Do NOT set DWMWA_NCRENDERING_POLICY = DWMNCRP_DISABLED here, as that
            // causes Windows to flash a classic non-client border frame.
            extern "system" {
                fn LoadLibraryW(lpLibFileName: *const u16) -> isize;
                fn GetProcAddress(hModule: isize, lpProcName: *const u8) -> *const core::ffi::c_void;
            }
            let dll: Vec<u16> = "dwmapi.dll\0".encode_utf16().collect();
            let dwm = LoadLibraryW(dll.as_ptr());
            if dwm != 0 {
                let proc = GetProcAddress(dwm, b"DwmSetWindowAttribute\0".as_ptr());
                if !proc.is_null() {
                    type FnDwmSetWindowAttribute = unsafe extern "system" fn(
                        hwnd: HWND,
                        dw_attribute: u32,
                        pv_attribute: *const core::ffi::c_void,
                        cb_attribute: u32,
                    ) -> i32;
                    let set_attr: FnDwmSetWindowAttribute = std::mem::transmute(proc);
                    // DWMWA_WINDOW_CORNER_PREFERENCE = 33, DWMWCP_DONOTROUND = 1
                    let corner: i32 = 1;
                    let _ = set_attr(
                        h,
                        33,
                        &corner as *const i32 as *const core::ffi::c_void,
                        std::mem::size_of::<i32>() as u32,
                    );
                }
            }

            let _ = SetWindowPos(
                h,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_FRAMECHANGED,
            );
        }
    }

    pub fn show_window(raw: isize) {
        if raw == 0 {
            return;
        }
        unsafe {
            let h = hwnd(raw);
            let _ = ShowWindow(h, SW_SHOW);
            let _ = SetWindowPos(h, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE);
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

    /// Returns `(editable_for_paste, active_input)`.
    /// - `editable_for_paste`: defaults to `true` when UIA cannot tell, so
    ///   in-place pasting still works in custom/canvas editors when text is selected.
    /// - `active_input`: only `true` when UIA positively confirms an editable
    ///   text control (or Win32 reports a live caret), so we never send Ctrl+A
    ///   outside an active text field.
    pub fn wait_detailed(self) -> (bool, bool) {
        let left = Duration::from_millis(700).saturating_sub(self.started.elapsed());
        match self.rx.recv_timeout(left) {
            Ok(Some(editable)) => (editable, editable),
            Ok(None) | Err(_) => (true, caret_rect(self.target).is_some()),
        }
    }
}

#[cfg(windows)]
fn variant_bool(v: &windows::Win32::System::Variant::VARIANT) -> Option<bool> {
    use windows::Win32::System::Variant::VT_BOOL;
    unsafe {
        if v.Anonymous.Anonymous.vt == VT_BOOL {
            Some(v.Anonymous.Anonymous.Anonymous.boolVal.as_bool())
        } else {
            None
        }
    }
}

#[cfg(windows)]
unsafe fn uia_element_editable(
    el: &windows::Win32::UI::Accessibility::IUIAutomationElement,
) -> Option<bool> {
    use windows::Win32::UI::Accessibility::{
        IUIAutomationTextPattern, IUIAutomationValuePattern, UIA_DocumentControlTypeId,
        UIA_EditControlTypeId, UIA_IsReadOnlyAttributeId, UIA_IsTextEditPatternAvailablePropertyId,
        UIA_TextPatternId, UIA_ValuePatternId,
    };

    let kind = el.CurrentControlType().ok();

    // 1. Check TextPattern's selection IsReadOnly attribute first: this is the
    //    most accurate signal across browsers, rich editors, Word, Win11 Notepad,
    //    and PDF viewers.
    if let Ok(tp) = el.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId) {
        if let Ok(sel) = tp.GetSelection() {
            if sel.Length().unwrap_or(0) > 0 {
                if let Ok(range) = sel.GetElement(0) {
                    if let Ok(val) = range.GetAttributeValue(UIA_IsReadOnlyAttributeId) {
                        if let Some(ro) = variant_bool(&val) {
                            return Some(!ro);
                        }
                    }
                }
            }
        }
    }

    // 2. ValuePattern reports IsReadOnly for standard inputs/textareas/Win32 edits.
    //    Skip Document controls because browsers expose the page URL as a
    //    read-only ValuePattern even inside canvas editors like Google Docs.
    if kind != Some(UIA_DocumentControlTypeId) {
        if let Ok(value) = el.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId) {
            if let Ok(ro) = value.CurrentIsReadOnly() {
                return Some(!ro.as_bool());
            }
        }
    }

    // 3. Edit controls are inherently editable text inputs.
    if kind == Some(UIA_EditControlTypeId) {
        return Some(true);
    }

    // 4. TextEditPattern is exposed by editable text hosts (contenteditable,
    //    RichEdit, WinUI text controls).
    if let Ok(val) = el.GetCurrentPropertyValue(UIA_IsTextEditPatternAvailablePropertyId) {
        if variant_bool(&val) == Some(true) {
            return Some(true);
        }
    }

    None
}

#[cfg(windows)]
fn uia_focus_editable() -> Option<bool> {
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
    };
    use windows::Win32::UI::Accessibility::{
        CUIAutomation, IUIAutomation, UIA_WindowControlTypeId,
    };
    unsafe {
        let initialized = CoInitializeEx(None, COINIT_MULTITHREADED).is_ok();
        let result = (|| -> windows::core::Result<Option<bool>> {
            let uia: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)?;
            let mut cur = uia.GetFocusedElement()?;
            if let Some(editable) = uia_element_editable(&cur) {
                return Ok(Some(editable));
            }
            // Focus can land on a child text/paragraph node inside an editable
            // contenteditable container or inside a read-only document; walk up
            // a few ancestors to find the enclosing text host.
            if let Ok(walker) = uia.ControlViewWalker() {
                for _ in 0..4 {
                    let Ok(parent) = walker.GetParentElement(&cur) else { break };
                    if parent.CurrentControlType().ok() == Some(UIA_WindowControlTypeId) {
                        break;
                    }
                    if let Some(editable) = uia_element_editable(&parent) {
                        return Ok(Some(editable));
                    }
                    cur = parent;
                }
            }
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
    pub fn is_terminal_window(_: isize) -> bool { false }
    pub fn last_input_tick() -> u32 { 0 }
    pub fn capture_selection(_: isize) -> Option<String> { None }
    pub fn verify_selection(_: isize) -> Option<String> { None }
    pub fn capture_all_in_field(_: isize) -> Option<String> { None }
    pub fn focus_window(_: isize) -> bool { false }
    pub fn paste_into(_: isize, _: &str) -> Result<(), String> {
        Err("Pasting into other apps is only supported on Windows for now.".into())
    }
    pub fn caret_rect(_: isize) -> Option<(i32, i32, i32, i32)> { None }
    pub fn cursor_pos() -> (i32, i32) { (200, 200) }
    pub fn work_area_at(_: i32, _: i32) -> (i32, i32, i32, i32) { (0, 0, 1440, 900) }
    pub fn monitor_scale_at(_: i32, _: i32) -> Option<f64> { None }
    pub fn set_no_activate(_: isize, _: bool) {}
    pub fn configure_popup_window(_: isize) {}
    pub fn show_window(_: isize) {}
    pub fn hide_window(_: isize) {}
    pub fn set_bounds(_: isize, _: i32, _: i32, _: i32, _: i32) {}
    pub struct EditableCheck;
    impl EditableCheck {
        pub fn start(_: isize) -> Self { EditableCheck }
        pub fn wait_detailed(self) -> (bool, bool) { (true, false) }
    }
    pub fn show_no_activate(_: isize) {}
}

#[cfg(not(windows))]
pub use imp_other::*;
