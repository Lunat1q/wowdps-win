//! The running daemon's tray icon on Windows: a left click opens the meter;
//! the right-click menu opens it or quits wowdps — every wowdps program
//! running (the window, the overlay, a TUI, the MCP servers, history and
//! extraction runs), then the daemon itself. Plain Win32 through `extern "system"`, no crate.

use std::ffi::c_void;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::time::Duration;

type Hwnd = isize;
type WndProc = extern "system" fn(Hwnd, u32, usize, isize) -> isize;

const WM_NULL: u32 = 0x0000;
const WM_LBUTTONUP: u32 = 0x0202;
const WM_RBUTTONUP: u32 = 0x0205;
/// What the icon sends its window (`WM_APP + 1`).
const WM_TRAY: u32 = 0x8001;
const NIM_ADD: u32 = 0;
const NIM_DELETE: u32 = 2;
const NIF_MESSAGE: u32 = 0x1;
const NIF_ICON: u32 = 0x2;
const NIF_TIP: u32 = 0x4;
const MF_STRING: u32 = 0x0;
const MF_SEPARATOR: u32 = 0x800;
const TPM_RIGHTBUTTON: u32 = 0x2;
const TPM_RETURNCMD: u32 = 0x100;
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const ID_OPEN: usize = 1;
const ID_QUIT: usize = 2;

#[repr(C)]
struct WndClassW {
    style: u32,
    wnd_proc: WndProc,
    cls_extra: i32,
    wnd_extra: i32,
    instance: isize,
    icon: isize,
    cursor: isize,
    background: isize,
    menu_name: *const u16,
    class_name: *const u16,
}

#[repr(C)]
#[derive(Default)]
struct Point {
    x: i32,
    y: i32,
}

#[repr(C)]
#[derive(Default)]
struct Msg {
    hwnd: Hwnd,
    message: u32,
    w_param: usize,
    l_param: isize,
    time: u32,
    pt: Point,
    private: u32,
}

#[repr(C)]
struct NotifyIconDataW {
    size: u32,
    hwnd: Hwnd,
    id: u32,
    flags: u32,
    callback_message: u32,
    icon: isize,
    tip: [u16; 128],
    state: u32,
    state_mask: u32,
    info: [u16; 256],
    version: u32,
    info_title: [u16; 64],
    info_flags: u32,
    guid: [u8; 16],
    balloon_icon: isize,
}

#[link(name = "user32")]
unsafe extern "system" {
    fn RegisterClassW(class: *const WndClassW) -> u16;
    #[allow(clippy::too_many_arguments)]
    fn CreateWindowExW(
        ex_style: u32,
        class: *const u16,
        title: *const u16,
        style: u32,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        parent: Hwnd,
        menu: isize,
        instance: isize,
        param: *const c_void,
    ) -> Hwnd;
    fn DefWindowProcW(hwnd: Hwnd, msg: u32, w: usize, l: isize) -> isize;
    fn GetMessageW(msg: *mut Msg, hwnd: Hwnd, min: u32, max: u32) -> i32;
    fn TranslateMessage(msg: *const Msg) -> i32;
    fn DispatchMessageW(msg: *const Msg) -> isize;
    fn CreatePopupMenu() -> isize;
    fn AppendMenuW(menu: isize, flags: u32, id: usize, text: *const u16) -> i32;
    fn TrackPopupMenu(
        menu: isize,
        flags: u32,
        x: i32,
        y: i32,
        reserved: i32,
        hwnd: Hwnd,
        rect: *const c_void,
    ) -> i32;
    fn DestroyMenu(menu: isize) -> i32;
    fn SetForegroundWindow(hwnd: Hwnd) -> i32;
    fn GetCursorPos(pt: *mut Point) -> i32;
    fn PostMessageW(hwnd: Hwnd, msg: u32, w: usize, l: isize) -> i32;
    fn LoadImageW(
        instance: isize,
        name: *const u16,
        kind: u32,
        w: i32,
        h: i32,
        flags: u32,
    ) -> isize;
    fn GetSystemMetrics(index: i32) -> i32;
    fn EnumWindows(each: extern "system" fn(Hwnd, isize) -> i32, param: isize) -> i32;
    fn GetWindowThreadProcessId(hwnd: Hwnd, pid: *mut u32) -> u32;
    fn GetWindowTextW(hwnd: Hwnd, text: *mut u16, len: i32) -> i32;
    fn IsWindowVisible(hwnd: Hwnd) -> i32;
    fn IsIconic(hwnd: Hwnd) -> i32;
    fn ShowWindow(hwnd: Hwnd, cmd: i32) -> i32;
}

#[link(name = "shell32")]
unsafe extern "system" {
    fn Shell_NotifyIconW(message: u32, data: *const NotifyIconDataW) -> i32;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetModuleHandleW(name: *const u16) -> isize;
    fn OpenProcess(access: u32, inherit: i32, pid: u32) -> isize;
    fn QueryFullProcessImageNameW(process: isize, flags: u32, name: *mut u16, len: *mut u32)
    -> i32;
    fn CloseHandle(handle: isize) -> i32;
}

const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
const SW_RESTORE: i32 = 9;
const IMAGE_ICON: u32 = 1;
const SM_CXSMICON: i32 = 49;
const SM_CYSMICON: i32 = 50;

/// The icon's window, for [`remove`] from the daemon's thread (0: none).
static WINDOW: AtomicIsize = AtomicIsize::new(0);

/// Show the icon from a thread of its own. It waits a moment first: a
/// daemon that loses the lockfile to a running one exits before then and
/// never flashes a second icon.
pub fn spawn() {
    let _ = std::thread::Builder::new().name("tray".into()).spawn(|| {
        std::thread::sleep(Duration::from_millis(500));
        run();
    });
}

/// Take the icon down (the daemon is leaving): a process that exits with
/// its icon up leaves a ghost in the tray until the pointer passes over it.
pub fn remove() {
    let hwnd = WINDOW.swap(0, Ordering::SeqCst);
    if hwnd != 0 {
        let data = icon_data(hwnd, 0);
        // SAFETY: `data` is a valid NOTIFYICONDATAW for the call's length.
        unsafe { Shell_NotifyIconW(NIM_DELETE, &data) };
    }
}

fn run() {
    let class = wide("wowdps-tray");
    // SAFETY: plain Win32 calls with valid, NUL-terminated strings and
    // structs that outlive each call; the window lives as long as the loop.
    unsafe {
        let instance = GetModuleHandleW(std::ptr::null());
        let wc = WndClassW {
            style: 0,
            wnd_proc,
            cls_extra: 0,
            wnd_extra: 0,
            instance,
            icon: 0,
            cursor: 0,
            background: 0,
            menu_name: std::ptr::null(),
            class_name: class.as_ptr(),
        };
        if RegisterClassW(&wc) == 0 {
            return;
        }
        // A plain hidden window, not a message-only one: the menu needs a
        // window that can come to the foreground.
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            class.as_ptr(),
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            instance,
            std::ptr::null(),
        );
        if hwnd == 0 {
            return;
        }
        // The app icon linked into this exe (build.rs), at the small size.
        let icon = LoadImageW(
            instance,
            std::ptr::without_provenance(1),
            IMAGE_ICON,
            GetSystemMetrics(SM_CXSMICON),
            GetSystemMetrics(SM_CYSMICON),
            0,
        );
        let data = icon_data(hwnd, icon);
        if Shell_NotifyIconW(NIM_ADD, &data) == 0 {
            return;
        }
        WINDOW.store(hwnd, Ordering::SeqCst);
        let mut msg = Msg::default();
        while GetMessageW(&mut msg, 0, 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

extern "system" fn wnd_proc(hwnd: Hwnd, msg: u32, w: usize, l: isize) -> isize {
    if msg == WM_TRAY {
        match (l & 0xffff) as u32 {
            WM_LBUTTONUP => open_meter(),
            WM_RBUTTONUP => menu(hwnd),
            _ => {}
        }
        return 0;
    }
    // SAFETY: the default handling of a message this window was sent.
    unsafe { DefWindowProcW(hwnd, msg, w, l) }
}

fn menu(hwnd: Hwnd) {
    let open_label = wide("Open meter");
    let quit_label = wide("Quit wowdps");
    // SAFETY: a menu built, shown and destroyed here, on the window's own
    // thread; the strings outlive the calls.
    let picked = unsafe {
        let menu = CreatePopupMenu();
        AppendMenuW(menu, MF_STRING, ID_OPEN, open_label.as_ptr());
        AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
        AppendMenuW(menu, MF_STRING, ID_QUIT, quit_label.as_ptr());
        let mut pt = Point::default();
        GetCursorPos(&mut pt);
        // Without it the menu stays up after a click elsewhere.
        SetForegroundWindow(hwnd);
        let picked = TrackPopupMenu(
            menu,
            TPM_RIGHTBUTTON | TPM_RETURNCMD,
            pt.x,
            pt.y,
            0,
            hwnd,
            std::ptr::null(),
        );
        PostMessageW(hwnd, WM_NULL, 0, 0);
        DestroyMenu(menu);
        picked
    };
    match picked as usize {
        ID_OPEN => open_meter(),
        ID_QUIT => quit(),
        _ => {}
    }
}

/// Bring the meter window forward when one is open, else start one.
fn open_meter() {
    let gui = crate::find_bin("wowdps-gui");
    match meter_window(&gui) {
        // SAFETY: a window handle just found, restored and raised; a
        // window gone since fails the calls harmlessly.
        Some(hwnd) => unsafe {
            if IsIconic(hwnd) != 0 {
                ShowWindow(hwnd, SW_RESTORE);
            }
            SetForegroundWindow(hwnd);
        },
        None => {
            let _ = std::process::Command::new(gui).spawn();
        }
    }
}

/// The visible window titled `wowdps` of a process running `gui`: the
/// meter window. The overlay's window has no title, and a title alone
/// would match a folder of that name open in Explorer.
fn meter_window(gui: &std::path::Path) -> Option<Hwnd> {
    extern "system" fn each(hwnd: Hwnd, found: isize) -> i32 {
        // SAFETY: `found` is the `Vec` `meter_window` passes, alive for
        // the whole enumeration.
        let found = unsafe { &mut *(found as *mut Vec<Hwnd>) };
        let mut text = [0u16; 16];
        // SAFETY: reads of a live window into a buffer of the given size.
        let titled = unsafe {
            IsWindowVisible(hwnd) != 0
                && GetWindowTextW(hwnd, text.as_mut_ptr(), text.len() as i32) == 6
        };
        if titled && text.iter().take(6).copied().eq("wowdps".encode_utf16()) {
            found.push(hwnd);
        }
        1
    }
    let mut titled: Vec<Hwnd> = Vec::new();
    // SAFETY: the callback only reads windows and pushes onto `titled`.
    unsafe { EnumWindows(each, &mut titled as *mut Vec<Hwnd> as isize) };
    titled
        .into_iter()
        .find(|&hwnd| image_of(hwnd).is_some_and(|p| p.eq_ignore_ascii_case(gui.as_os_str())))
}

/// The full path of the program behind `hwnd`.
fn image_of(hwnd: Hwnd) -> Option<std::ffi::OsString> {
    use std::os::windows::ffi::OsStringExt as _;
    let mut pid = 0u32;
    // SAFETY: plain queries; the process handle is closed before return.
    unsafe {
        GetWindowThreadProcessId(hwnd, &mut pid);
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process == 0 {
            return None;
        }
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(process, 0, buf.as_mut_ptr(), &mut len) != 0;
        CloseHandle(process);
        let name = buf.get(..len as usize)?;
        ok.then(|| std::ffi::OsString::from_wide(name))
    }
}

/// The programs Quit stops, wherever each runs from.
const PROGRAMS: &str = "wowdps,wowdps-gui,wowdps-mcp,wowdps-history,wowdps-extract";

/// Every other wowdps program first, from any folder (the window, the
/// overlay, a TUI, the MCP servers, a history query, an extraction): a
/// client left running would reconnect and start a daemon again. Then this
/// daemon, as `wowdps stop` asks it.
fn quit() {
    use std::os::windows::process::CommandExt as _;
    let script = format!(
        "Get-Process -Name {PROGRAMS} -ErrorAction SilentlyContinue | \
         Where-Object {{ $_.Id -ne {} }} | Stop-Process -Force",
        std::process::id()
    );
    let _ = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .creation_flags(CREATE_NO_WINDOW)
        .status();
    crate::ask_stop();
}

fn icon_data(hwnd: Hwnd, icon: isize) -> NotifyIconDataW {
    let mut tip = [0u16; 128];
    for (slot, c) in tip.iter_mut().zip("wowdps".encode_utf16()) {
        *slot = c;
    }
    NotifyIconDataW {
        size: std::mem::size_of::<NotifyIconDataW>() as u32,
        hwnd,
        id: 1,
        flags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
        callback_message: WM_TRAY,
        icon,
        tip,
        state: 0,
        state_mask: 0,
        info: [0; 256],
        version: 0,
        info_title: [0; 64],
        info_flags: 0,
        guid: [0; 16],
        balloon_icon: 0,
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_struct_is_the_size_windows_expects() {
        // NOTIFYICONDATAW on 64-bit Windows.
        assert_eq!(std::mem::size_of::<NotifyIconDataW>(), 976);
    }
}
