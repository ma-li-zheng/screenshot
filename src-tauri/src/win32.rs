//! 直接调用 Win32 API 的部分：枚举窗口、光标、模拟滚轮、排除截屏
use serde::Serialize;

#[derive(Clone, Copy, Serialize, Debug)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn intersects(&self, o: &Rect) -> bool {
        self.x < o.x + o.w && o.x < self.x + self.w && self.y < o.y + o.h && o.y < self.y + self.h
    }
}

/// 一个顶层窗口及其子窗口（屏幕物理坐标，按 Z 序从上到下）
#[derive(Clone, Serialize, Debug)]
pub struct WinInfo {
    pub rect: Rect,
    pub children: Vec<Rect>,
}

#[cfg(windows)]
mod imp {
    use super::*;
    use windows::core::BOOL;
    use windows::Win32::Foundation::{HWND, LPARAM, POINT, RECT};
    use windows::Win32::Graphics::Dwm::{
        DwmGetWindowAttribute, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS,
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_WHEEL, MOUSEINPUT,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumChildWindows, EnumWindows, GetAncestor, GetCursorPos, GetWindowThreadProcessId,
        SetForegroundWindow, WindowFromPoint, GA_ROOT, GetWindowLongW, GetWindowRect, IsIconic,
        IsWindowVisible, SetCursorPos, SetWindowDisplayAffinity, GWL_EXSTYLE,
        WDA_EXCLUDEFROMCAPTURE, WS_EX_TRANSPARENT,
    };

    fn to_rect(r: RECT) -> Rect {
        Rect {
            x: r.left,
            y: r.top,
            w: r.right - r.left,
            h: r.bottom - r.top,
        }
    }

    unsafe fn window_rect(hwnd: HWND) -> Option<Rect> {
        let mut r = RECT::default();
        let ok = DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            &mut r as *mut _ as *mut _,
            std::mem::size_of::<RECT>() as u32,
        )
        .is_ok();
        if !ok && GetWindowRect(hwnd, &mut r).is_err() {
            return None;
        }
        let rect = to_rect(r);
        (rect.w > 4 && rect.h > 4).then_some(rect)
    }

    unsafe extern "system" fn child_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let list = &mut *(lparam.0 as *mut Vec<Rect>);
        if IsWindowVisible(hwnd).as_bool() {
            let mut r = RECT::default();
            if GetWindowRect(hwnd, &mut r).is_ok() {
                let rect = to_rect(r);
                if rect.w >= 16 && rect.h >= 16 && list.len() < 500 {
                    list.push(rect);
                }
            }
        }
        BOOL(1)
    }

    unsafe extern "system" fn top_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let list = &mut *(lparam.0 as *mut Vec<WinInfo>);
        if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
            return BOOL(1);
        }
        let mut cloaked: u32 = 0;
        let _ = DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED,
            &mut cloaked as *mut _ as *mut _,
            std::mem::size_of::<u32>() as u32,
        );
        if cloaked != 0 {
            return BOOL(1);
        }
        let ex = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
        if ex & WS_EX_TRANSPARENT.0 != 0 {
            return BOOL(1);
        }
        if let Some(rect) = window_rect(hwnd) {
            let mut children = Vec::new();
            let _ = EnumChildWindows(
                Some(hwnd),
                Some(child_proc),
                LPARAM(&mut children as *mut _ as isize),
            );
            list.push(WinInfo { rect, children });
        }
        BOOL(1)
    }

    pub fn enum_windows() -> Vec<WinInfo> {
        let mut list: Vec<WinInfo> = Vec::new();
        unsafe {
            let _ = EnumWindows(Some(top_proc), LPARAM(&mut list as *mut _ as isize));
        }
        list
    }

    pub fn cursor_pos() -> (i32, i32) {
        let mut p = POINT::default();
        unsafe {
            let _ = GetCursorPos(&mut p);
        }
        (p.x, p.y)
    }

    pub fn set_cursor_pos(x: i32, y: i32) {
        unsafe {
            let _ = SetCursorPos(x, y);
        }
    }

    /// 向光标下方的窗口发送滚轮，delta < 0 表示向下滚
    pub fn wheel(delta: i32) {
        let input = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx: 0,
                    dy: 0,
                    mouseData: delta as u32,
                    dwFlags: MOUSEEVENTF_WHEEL,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        unsafe {
            SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
        }
    }

    /// 把 (x, y) 下方的其他程序窗口切到前台，保证滚轮作用到它
    pub fn activate_window_at(x: i32, y: i32) {
        unsafe {
            let h = WindowFromPoint(POINT { x, y });
            if h.is_invalid() {
                return;
            }
            let root = GetAncestor(h, GA_ROOT);
            let mut pid = 0u32;
            GetWindowThreadProcessId(root, Some(&mut pid));
            if pid != std::process::id() {
                let _ = SetForegroundWindow(root);
            }
        }
    }

    /// 把文件放到剪贴板（与资源管理器复制文件相同，可直接粘贴到微信、飞书、文件夹）
    pub fn copy_files_to_clipboard(paths: &[std::path::PathBuf]) -> anyhow::Result<()> {
        use windows::Win32::Foundation::HANDLE;
        use windows::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData};
        use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
        const CF_HDROP: u32 = 15;
        /// DROPFILES 结构
        #[repr(C)]
        struct DropFiles {
            p_files: u32,
            pt: POINT,
            f_nc: i32,
            f_wide: i32,
        }
        let mut wide: Vec<u16> = Vec::new();
        for p in paths {
            wide.extend(p.as_os_str().encode_wide_compat());
            wide.push(0);
        }
        wide.push(0);
        let header = std::mem::size_of::<DropFiles>();
        unsafe {
            let h = GlobalAlloc(GMEM_MOVEABLE, header + wide.len() * 2)?;
            let ptr = GlobalLock(h) as *mut u8;
            if ptr.is_null() {
                anyhow::bail!("无法分配剪贴板内存");
            }
            std::ptr::write(
                ptr as *mut DropFiles,
                DropFiles {
                    p_files: header as u32,
                    pt: POINT::default(),
                    f_nc: 0,
                    f_wide: 1,
                },
            );
            std::ptr::copy_nonoverlapping(wide.as_ptr() as *const u8, ptr.add(header), wide.len() * 2);
            let _ = GlobalUnlock(h);
            OpenClipboard(None)?;
            let result = EmptyClipboard().and_then(|_| SetClipboardData(CF_HDROP, Some(HANDLE(h.0))).map(|_| ()));
            let _ = CloseClipboard();
            result?;
        }
        Ok(())
    }

    trait EncodeWide {
        fn encode_wide_compat(&self) -> Vec<u16>;
    }
    impl EncodeWide for std::ffi::OsStr {
        fn encode_wide_compat(&self) -> Vec<u16> {
            use std::os::windows::ffi::OsStrExt;
            self.encode_wide().collect()
        }
    }

    /// 让窗口不出现在任何截屏/录屏结果里（Win10 2004+）
    pub fn exclude_from_capture(hwnd: isize) {
        // 调试用：设置该环境变量后可以截到长截图控制条
        if std::env::var_os("SCREENSHOT_DEBUG_NO_EXCLUDE").is_some() {
            return;
        }
        unsafe {
            let _ = SetWindowDisplayAffinity(HWND(hwnd as *mut _), WDA_EXCLUDEFROMCAPTURE);
        }
    }
}

#[cfg(windows)]
pub use imp::*;
