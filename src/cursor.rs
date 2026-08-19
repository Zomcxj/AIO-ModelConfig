#![cfg(target_os = "windows")]

use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::sync::OnceLock;
use std::ffi::c_void;
use std::io::Write;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    CreateCompatibleBitmap, CreateDIBSection, DeleteObject, GetDC, ReleaseDC, DIB_RGB_COLORS,
    RGBQUAD,
};
use windows_sys::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass, SUBCLASSPROC};
use windows_sys::Win32::UI::WindowsAndMessaging::{CreateIconIndirect, ICONINFO, SetCursor};

static DEBUG_FILE: OnceLock<std::sync::Mutex<Option<std::fs::File>>> = OnceLock::new();

fn debug_log(msg: &str) {
    let guard = DEBUG_FILE.get_or_init(|| std::sync::Mutex::new(None));
    let mut lock = guard.lock().unwrap();
    if lock.is_none() {
        *lock = std::fs::File::create("D:/VsPro/opencode-tool/cursor_debug.log").ok();
    }
    if let Some(ref mut f) = *lock {
        let _ = f.write_all(msg.as_bytes());
        let _ = f.flush();
    }
}

static USE_CUSTOM_CURSOR: AtomicBool = AtomicBool::new(false);
static CURSOR_HANDLE: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

pub fn set_custom_cursor_active(active: bool) {
    USE_CUSTOM_CURSOR.store(active, Ordering::Relaxed);
    if active {
        let ptr = CURSOR_HANDLE.load(Ordering::Relaxed);
        if !ptr.is_null() {
            unsafe {
                debug_log(&format!("Win32 SetCursor custom {:?}\r\n", ptr));
                SetCursor(ptr);
            }
        }
    }
}

pub fn is_custom_cursor_active() -> bool {
    USE_CUSTOM_CURSOR.load(Ordering::Relaxed)
}

const GRAB_CURSOR_W: i32 = 20;
const GRAB_CURSOR_H: i32 = 20;
const GRAB_CURSOR_HX: i32 = 10;
const GRAB_CURSOR_HY: i32 = 10;

const GRAB_CURSOR_RGBA: &[u8] = include_bytes!("../assets/grab_rgba.bin");

/// Create a cursor from BGRA pixel data using Win32 GDI.
unsafe fn create_icon_from_rgba(rgba: &[u8], w: i32, h: i32, hx: i32, hy: i32) -> *mut c_void {
    let hdc = GetDC(std::ptr::null_mut());

    let bmi = windows_sys::Win32::Graphics::Gdi::BITMAPINFO {
        bmiHeader: windows_sys::Win32::Graphics::Gdi::BITMAPINFOHEADER {
            biSize: std::mem::size_of::<windows_sys::Win32::Graphics::Gdi::BITMAPINFOHEADER>()
                as u32,
            biWidth: w,
            biHeight: -h,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: 0,
            biSizeImage: 0,
            biXPelsPerMeter: 0,
            biYPelsPerMeter: 0,
            biClrUsed: 0,
            biClrImportant: 0,
        },
        bmiColors: [RGBQUAD {
            rgbBlue: 0,
            rgbGreen: 0,
            rgbRed: 0,
            rgbReserved: 0,
        }; 1],
    };

    let mut pixels: *mut c_void = std::ptr::null_mut();
    let hbm_color =
        CreateDIBSection(hdc, &bmi, DIB_RGB_COLORS, &mut pixels, std::ptr::null_mut(), 0);

    if !hbm_color.is_null() && !pixels.is_null() {
        let copy_len = ((w * h * 4) as usize).min(rgba.len());
        std::ptr::copy_nonoverlapping(rgba.as_ptr(), pixels as *mut u8, copy_len);
    }

    let hbm_mask = CreateCompatibleBitmap(hdc, w, h);

    let icon_info = ICONINFO {
        fIcon: 0,
        xHotspot: hx as u32,
        yHotspot: hy as u32,
        hbmMask: hbm_mask,
        hbmColor: hbm_color,
    };

    let hicon = CreateIconIndirect(&icon_info);

    DeleteObject(hbm_mask as _);
    DeleteObject(hbm_color as _);
    ReleaseDC(std::ptr::null_mut(), hdc);

    hicon
}

unsafe extern "system" fn cursor_subclass_proc(
    hwnd: HWND,
    u_msg: u32,
    w_param: WPARAM,
    l_param: LPARAM,
    _uid_subclass: usize,
    _dw_ref_data: usize,
) -> LRESULT {
    if u_msg == 0x0020 && (l_param as u32 & 0xffff) == 1 && is_custom_cursor_active() {
        let cursor_handle = _dw_ref_data as *mut c_void;
        SetCursor(cursor_handle);
        return 1;
    }
    DefSubclassProc(hwnd, u_msg, w_param, l_param)
}

pub unsafe fn init_grabbing_cursor(hwnd: HWND) {
    debug_log(&format!("init_grabbing_cursor hwnd={:?}\r\n", hwnd));
    let hicon = create_icon_from_rgba(
        GRAB_CURSOR_RGBA,
        GRAB_CURSOR_W,
        GRAB_CURSOR_H,
        GRAB_CURSOR_HX,
        GRAB_CURSOR_HY,
    );
    CURSOR_HANDLE.store(hicon, Ordering::Relaxed);
    debug_log(&format!("cursor created hicon={:?}\r\n", hicon));

    let subclass_proc: SUBCLASSPROC = Some(cursor_subclass_proc);
    let ret = SetWindowSubclass(hwnd, subclass_proc, 1, hicon as usize);
    debug_log(&format!("SetWindowSubclass ret={}\r\n", ret));
}
