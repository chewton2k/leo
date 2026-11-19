use std::sync::{mpsc::Sender, Arc, Mutex, Once};
use std::thread;
use std::time::Instant;

use log::{info, warn};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{
    SetLastError, ERROR_SUCCESS, HANDLE, HGLOBAL, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM,
};

use super::{evaluate, keep_waiting, send_chord, TxState, WaitDecision};
use windows::Win32::Foundation::GlobalFree;
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData, GetClipboardOwner,
    GetClipboardSequenceNumber, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Memory::{
    GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE,
};
use windows::Win32::System::Ole::{
    CF_BITMAP, CF_DSPBITMAP, CF_DSPENHMETAFILE, CF_DSPMETAFILEPICT, CF_ENHMETAFILE,
    CF_METAFILEPICT, CF_OWNERDISPLAY, CF_PALETTE, CF_UNICODETEXT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CopyImage, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW,
    GetWindowLongPtrW, KillTimer, PostQuitMessage, RegisterClassW, SetTimer, SetWindowLongPtrW,
    GDI_IMAGE_TYPE, GWLP_USERDATA, HWND_MESSAGE, IMAGE_FLAGS, MSG, WINDOW_EX_STYLE, WINDOW_STYLE,
    WM_DESTROYCLIPBOARD, WM_RENDERALLFORMATS, WM_RENDERFORMAT, WM_TIMER, WNDCLASSW,
};

const CLASS_NAME: PCWSTR = w!("LeoPasteTxWindow");
const TIMER_ID: usize = 1;
const TIMER_INTERVAL_MS: u32 = 25;

const IMAGE_BITMAP_TYPE: GDI_IMAGE_TYPE = GDI_IMAGE_TYPE(0);
const LR_CREATEDIBSECTION_FLAG: IMAGE_FLAGS = IMAGE_FLAGS(0x2000);

enum SavedData {
    Memory(Vec<u8>),
    Bitmap(usize),
    Enhanced(usize),
    Palette(usize),
    Metafile(usize),
}
struct SavedFormat {
    format: u32,
    data: SavedData,
}
impl Drop for SavedData {
    fn drop(&mut self) {
        unsafe {
            use windows::Win32::Graphics::Gdi::*;
            match self {
                Self::Bitmap(raw) | Self::Palette(raw) if *raw != 0 => {
                    let _ = DeleteObject(HGDIOBJ(*raw as *mut _));
                }
                Self::Enhanced(raw) if *raw != 0 => {
                    let _ = DeleteEnhMetaFile(Some(HENHMETAFILE(*raw as *mut _)));
                }
                Self::Metafile(raw) if *raw != 0 => {
                    let hg = HGLOBAL(*raw as *mut _);
                    let ptr =
                        GlobalLock(hg) as *const windows::Win32::System::DataExchange::METAFILEPICT;
                    if !ptr.is_null() {
                        let _ = DeleteMetaFile((*ptr).hMF);
                        let _ = GlobalUnlock(hg);
                    }
                    let _ = GlobalFree(Some(hg));
                }
                _ => {}
            }
        }
    }
}

pub(super) struct WinTxShared {
    state: Mutex<TxState>,
    text: String,
    _activity: leo_core::activity::Lease,
    current: Arc<dyn Fn() -> bool + Send + Sync>,
    snapshot: Mutex<Vec<SavedFormat>>,
    sequence: Mutex<u32>,
    owner: std::sync::atomic::AtomicUsize,
    own: std::sync::atomic::AtomicBool,
}

static PENDING: Mutex<Option<Arc<WinTxShared>>> = Mutex::new(None);

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

unsafe fn shared_ptr(hwnd: HWND) -> *const WinTxShared {
    GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const WinTxShared
}

unsafe fn render_text(shared: &WinTxShared) {
    let wide_text: Vec<u16> = shared
        .text
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let Ok(hg) = GlobalAlloc(GMEM_MOVEABLE, wide_text.len() * 2) else {
        return;
    };
    let ptr = GlobalLock(hg) as *mut u16;
    if ptr.is_null() {
        let _ = GlobalFree(Some(hg));
        return;
    }
    std::ptr::copy_nonoverlapping(wide_text.as_ptr(), ptr, wide_text.len());
    let _ = GlobalUnlock(hg);
    if SetClipboardData(CF_UNICODETEXT.0 as u32, Some(HANDLE(hg.0))).is_err() {
        let _ = GlobalFree(Some(hg));
    }
}

unsafe extern "system" fn paste_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let shared = shared_ptr(hwnd);
    match msg {
        WM_RENDERFORMAT => {
            if !shared.is_null() {
                let shared = &*shared;
                if let Ok(mut st) = shared.state.lock() {
                    st.record_receipt(Instant::now());
                }
                if wparam.0 as u32 == CF_UNICODETEXT.0 as u32 {
                    render_text(shared);
                }
            }
            LRESULT(0)
        }
        WM_RENDERALLFORMATS => {
            if !shared.is_null() {
                let shared = &*shared;
                if OpenClipboard(Some(hwnd)).is_ok() {
                    if GetClipboardOwner()
                        .map(|owner| owner == hwnd)
                        .unwrap_or(false)
                    {
                        render_text(shared);
                    }
                    let _ = CloseClipboard();
                }
            }
            LRESULT(0)
        }
        WM_DESTROYCLIPBOARD => {
            if !shared.is_null() {
                if let Ok(mut st) = (&*shared).state.lock() {
                    st.ownership_lost = true;
                }
            }
            LRESULT(0)
        }
        WM_TIMER => {
            if !shared.is_null() {
                on_timer(hwnd, &*shared);
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn ensure_window_class(hinstance: HINSTANCE) {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let wc = WNDCLASSW {
            lpfnWndProc: Some(paste_wnd_proc),
            hInstance: hinstance,
            lpszClassName: CLASS_NAME,
            ..Default::default()
        };
        unsafe {
            RegisterClassW(&wc);
        }
    });
}

pub(super) fn flush_pending() {
    let previous = match PENDING.lock() {
        Ok(mut slot) => slot.take(),
        Err(_) => None,
    };
    let Some(previous) = previous else {
        return;
    };
    match previous.state.lock() {
        Ok(mut st) => st.cancelled = true,
        Err(_) => return,
    }
    let hwnd = HWND(previous.owner.load(std::sync::atomic::Ordering::Acquire) as *mut _);
    let _ = unsafe {
        windows::Win32::UI::WindowsAndMessaging::SendMessageW(
            hwnd,
            WM_TIMER,
            Some(WPARAM(TIMER_ID)),
            None,
        )
    };
}

unsafe fn open_owned(shared: &WinTxShared) -> bool {
    let hwnd = HWND(shared.owner.load(std::sync::atomic::Ordering::Acquire) as *mut _);
    (0..10).any(|attempt| {
        if attempt > 0 {
            thread::sleep(std::time::Duration::from_millis(20));
        }
        OpenClipboard(Some(hwnd)).is_ok()
    })
}

fn preserved_format(format: u32) -> bool {
    !(0x0200..=0x02FF).contains(&format)
}

unsafe fn settle_clipboard(shared: &WinTxShared) {
    let preserve_transcript = shared
        .state
        .lock()
        .map(|st| st.injection_failed)
        .unwrap_or(false);
    if !preserve_transcript {
        restore_snapshot(shared);
        return;
    }
    if !open_owned(shared) {
        warn!("[reliable-paste] could not open clipboard to leave transcript");
        return;
    }
    let sequence = *shared.sequence.lock().unwrap_or_else(|e| e.into_inner());
    if GetClipboardSequenceNumber() != sequence
        || GetClipboardOwner().ok().map(|h| h.0 as usize)
            != Some(shared.owner.load(std::sync::atomic::Ordering::Acquire))
    {
        let _ = CloseClipboard();
        return;
    }
    render_text(shared);
    let _ = CloseClipboard();
    super::copied();
    info!("[reliable-paste] left transcript on clipboard as plain text");
}

unsafe fn restore_snapshot(shared: &WinTxShared) {
    if !open_owned(shared) {
        warn!("[reliable-paste] could not open clipboard to restore");
        return;
    }
    let sequence = *shared.sequence.lock().unwrap_or_else(|e| e.into_inner());
    if GetClipboardSequenceNumber() != sequence
        || GetClipboardOwner().ok().map(|h| h.0 as usize)
            != Some(shared.owner.load(std::sync::atomic::Ordering::Acquire))
    {
        let _ = CloseClipboard();
        return;
    }
    let mut formats = match shared.snapshot.lock() {
        Ok(mut saved) => std::mem::take(&mut *saved),
        Err(_) => {
            let _ = CloseClipboard();
            return;
        }
    };
    let _ = EmptyClipboard();
    let concealed = RegisterClipboardFormatW(w!("ExcludeClipboardContentFromMonitorProcessing"));
    let own = shared.own.load(std::sync::atomic::Ordering::Acquire);
    if !own && formats.iter().any(|saved| saved.format == concealed) {
        formats.clear();
        info!("[reliable-paste] previous clipboard was concealed; cleared instead of restoring");
    }
    for saved in &mut formats {
        let handle = match &saved.data {
            SavedData::Memory(data) => {
                let Ok(hg) = GlobalAlloc(GMEM_MOVEABLE, data.len().max(1)) else {
                    continue;
                };
                let ptr = GlobalLock(hg) as *mut u8;
                if ptr.is_null() {
                    let _ = GlobalFree(Some(hg));
                    continue;
                }
                std::ptr::copy_nonoverlapping(data.as_ptr(), ptr, data.len());
                let _ = GlobalUnlock(hg);
                HANDLE(hg.0)
            }
            SavedData::Bitmap(raw)
            | SavedData::Enhanced(raw)
            | SavedData::Palette(raw)
            | SavedData::Metafile(raw) => HANDLE(*raw as *mut _),
        };
        if SetClipboardData(saved.format, Some(handle)).is_ok() {
            match &mut saved.data {
                SavedData::Memory(_) => {}
                SavedData::Bitmap(raw)
                | SavedData::Enhanced(raw)
                | SavedData::Palette(raw)
                | SavedData::Metafile(raw) => *raw = 0,
            }
        } else if matches!(saved.data, SavedData::Memory(_)) {
            let _ = GlobalFree(Some(HGLOBAL(handle.0)));
        }
    }
    let _ = CloseClipboard();
    if own {
        super::copied();
    }
}

unsafe fn snapshot_clipboard(hwnd: HWND, shared: &WinTxShared) -> Result<(), String> {
    use windows::Win32::Graphics::Gdi::*;
    use windows::Win32::System::DataExchange::METAFILEPICT;
    OpenClipboard(Some(hwnd)).map_err(|_| "The clipboard is busy. Try dictating again.")?;
    shared.own.store(
        super::own_copy(GetClipboardSequenceNumber() as i64),
        std::sync::atomic::Ordering::Release,
    );
    struct Open;
    impl Drop for Open {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseClipboard();
            }
        }
    }
    let _open = Open;
    let mut formats = Vec::new();
    let mut format = 0u32;
    let mut total = 0usize;
    loop {
        format = EnumClipboardFormats(format);
        if format == 0 {
            break;
        }
        if formats.len() >= super::MOST_ENTRIES || format == CF_OWNERDISPLAY.0 as u32 {
            return Err(super::TOO_MANY.into());
        }
        if !preserved_format(format) {
            continue;
        }
        let handle =
            GetClipboardData(format).map_err(|_| "A clipboard format could not be preserved.")?;
        let data = if format == CF_BITMAP.0 as u32 || format == CF_DSPBITMAP.0 as u32 {
            let mut bitmap = BITMAP::default();
            if GetObjectW(
                HGDIOBJ(handle.0),
                std::mem::size_of::<BITMAP>() as i32,
                Some((&mut bitmap as *mut BITMAP).cast()),
            ) == 0
            {
                return Err("The clipboard image could not be preserved.".into());
            }
            total = total.saturating_add(
                (bitmap.bmWidthBytes.unsigned_abs() as usize)
                    .saturating_mul(bitmap.bmHeight.unsigned_abs() as usize),
            );
            if total > super::MAX_CLIPBOARD_BYTES {
                return Err(super::TOO_LARGE.into());
            }
            let copy = CopyImage(handle, IMAGE_BITMAP_TYPE, 0, 0, LR_CREATEDIBSECTION_FLAG)
                .map_err(|_| "The clipboard image could not be preserved.")?;
            SavedData::Bitmap(copy.0 as usize)
        } else if format == CF_ENHMETAFILE.0 as u32 || format == CF_DSPENHMETAFILE.0 as u32 {
            let copy = CopyEnhMetaFileW(HENHMETAFILE(handle.0), PCWSTR::null());
            if copy.0.is_null() {
                return Err("The clipboard drawing could not be preserved.".into());
            }
            total = total.saturating_add(GetEnhMetaFileBits(HENHMETAFILE(handle.0), None) as usize);
            let data = SavedData::Enhanced(copy.0 as usize);
            if total > super::MAX_CLIPBOARD_BYTES {
                return Err(super::TOO_LARGE.into());
            }
            data
        } else if format == CF_PALETTE.0 as u32 {
            let count = GetPaletteEntries(HPALETTE(handle.0), 0, None) as usize;
            if count == 0 || count > 65535 {
                return Err("The clipboard palette could not be preserved.".into());
            }
            let mut data = vec![0u32; count + 1];
            data[0] = 0x300 | ((count as u32) << 16);
            let entries = std::slice::from_raw_parts_mut(
                data.as_mut_ptr().add(1).cast::<PALETTEENTRY>(),
                count,
            );
            if GetPaletteEntries(HPALETTE(handle.0), 0, Some(entries)) as usize != count {
                return Err("The clipboard palette could not be preserved.".into());
            }
            let copy = CreatePalette(data.as_ptr().cast::<LOGPALETTE>());
            if copy.0.is_null() {
                return Err("The clipboard palette could not be preserved.".into());
            }
            SavedData::Palette(copy.0 as usize)
        } else if format == CF_METAFILEPICT.0 as u32 || format == CF_DSPMETAFILEPICT.0 as u32 {
            let source = HGLOBAL(handle.0);
            let ptr = GlobalLock(source) as *const METAFILEPICT;
            if ptr.is_null() {
                return Err("The clipboard drawing could not be preserved.".into());
            }
            let mut picture = std::ptr::read_unaligned(ptr);
            let _ = GlobalUnlock(source);
            let copy = CopyMetaFileW(picture.hMF, PCWSTR::null());
            if copy.0.is_null() {
                return Err("The clipboard drawing could not be preserved.".into());
            }
            total = total.saturating_add(GetMetaFileBitsEx(picture.hMF, 0, None) as usize);
            if total > super::MAX_CLIPBOARD_BYTES {
                let _ = DeleteMetaFile(copy);
                return Err(super::TOO_LARGE.into());
            }
            picture.hMF = copy;
            let hg = match GlobalAlloc(GMEM_MOVEABLE, std::mem::size_of::<METAFILEPICT>()) {
                Ok(hg) => hg,
                Err(_) => {
                    let _ = DeleteMetaFile(copy);
                    return Err("The clipboard drawing could not be preserved.".into());
                }
            };
            let ptr = GlobalLock(hg) as *mut METAFILEPICT;
            if ptr.is_null() {
                let _ = GlobalFree(Some(hg));
                let _ = DeleteMetaFile(copy);
                return Err("The clipboard drawing could not be preserved.".into());
            }
            ptr.write(picture);
            let _ = GlobalUnlock(hg);
            SavedData::Metafile(hg.0 as usize)
        } else {
            let hg = HGLOBAL(handle.0);
            let size = GlobalSize(hg);
            total = total.saturating_add(size);
            if total > super::MAX_CLIPBOARD_BYTES {
                return Err(super::TOO_LARGE.into());
            }
            let ptr = GlobalLock(hg) as *const u8;
            if ptr.is_null() {
                return Err("A clipboard format could not be preserved.".into());
            }
            let data = std::slice::from_raw_parts(ptr, size).to_vec();
            let _ = GlobalUnlock(hg);
            SavedData::Memory(data)
        };
        formats.push(SavedFormat { format, data });
    }
    *shared.snapshot.lock().unwrap_or_else(|e| e.into_inner()) = formats;
    Ok(())
}

pub(super) fn count() -> i64 {
    unsafe { GetClipboardSequenceNumber() as i64 }
}

unsafe fn publish(hwnd: HWND) -> Result<u32, String> {
    OpenClipboard(Some(hwnd)).map_err(|e| format!("OpenClipboard failed: {e}"))?;
    let published = publish_formats();
    let closed = CloseClipboard();
    published?;
    closed.map_err(|e| format!("CloseClipboard failed: {e}"))?;
    Ok(GetClipboardSequenceNumber())
}

unsafe fn publish_formats() -> Result<(), String> {
    EmptyClipboard().map_err(|e| format!("EmptyClipboard failed: {e}"))?;

    for (name, value) in [
        ("ExcludeClipboardContentFromMonitorProcessing", 1u32),
        ("CanIncludeInClipboardHistory", 0u32),
        ("CanUploadToCloudClipboard", 0u32),
    ] {
        let name_wide = wide(name);
        let format = RegisterClipboardFormatW(PCWSTR(name_wide.as_ptr()));
        if format == 0 {
            continue;
        }
        if let Ok(hg) = GlobalAlloc(GMEM_MOVEABLE, std::mem::size_of::<u32>()) {
            let ptr = GlobalLock(hg) as *mut u32;
            if !ptr.is_null() {
                *ptr = value;
                let _ = GlobalUnlock(hg);
                if SetClipboardData(format, Some(HANDLE(hg.0))).is_err() {
                    let _ = GlobalFree(Some(hg));
                }
            } else {
                let _ = GlobalFree(Some(hg));
            }
        }
    }

    SetLastError(ERROR_SUCCESS);
    if let Err(e) = SetClipboardData(CF_UNICODETEXT.0 as u32, None) {
        if e.code().is_err() {
            return Err(format!("SetClipboardData failed: {e}"));
        }
    }
    Ok(())
}

fn on_timer(_hwnd: HWND, shared: &WinTxShared) {
    let now = Instant::now();
    let finish = {
        let mut st = match shared.state.lock() {
            Ok(st) => st,
            Err(_) => return,
        };
        if !keep_waiting(st.injected_at.is_some(), (shared.current)()) {
            st.cancelled = true;
        }
        if st.cancelled {
            true
        } else {
            match evaluate(&st, now) {
                WaitDecision::KeepWaiting => false,
                WaitDecision::Finish => {
                    st.cancelled = true;
                    true
                }
            }
        }
    };
    if !finish {
        return;
    }

    let (receipt, ownership_lost, injection_failed) = {
        let st = match shared.state.lock() {
            Ok(st) => st,
            Err(_) => return,
        };
        (
            st.any_receipt_after_injection(),
            st.ownership_lost,
            st.injection_failed,
        )
    };
    if ownership_lost {
        info!("[reliable-paste] settling: clipboard ownership lost");
    } else if receipt {
        info!("[reliable-paste] settling: reads went quiet");
    } else if injection_failed {
        info!("[reliable-paste] settling: chord injection failed, restoring quickly");
    } else {
        info!("[reliable-paste] settling: no read within timeout, restoring anyway");
    }

    let sequence = *shared.sequence.lock().unwrap_or_else(|e| e.into_inner());
    let still_ours = !ownership_lost && unsafe { GetClipboardSequenceNumber() } == sequence;
    if still_ours {
        unsafe { settle_clipboard(shared) };
    } else {
        info!("[reliable-paste] clipboard changed externally; leaving it untouched");
    }

    if let Ok(mut slot) = PENDING.lock() {
        let is_us = slot
            .as_ref()
            .map(|pending| std::ptr::eq(Arc::as_ptr(pending), shared))
            .unwrap_or(false);
        if is_us {
            *slot = None;
        }
    }

    unsafe {
        PostQuitMessage(0);
    }
}

unsafe fn destroy_window_and_shared(hwnd: HWND) {
    let ptr = shared_ptr(hwnd);
    let _ = DestroyWindow(hwnd);
    if !ptr.is_null() {
        drop(Arc::from_raw(ptr));
    }
}

fn pump_thread(shared: Arc<WinTxShared>, ready: Sender<Result<(), String>>) {
    unsafe {
        flush_pending();

        let hinstance = match GetModuleHandleW(PCWSTR::null()) {
            Ok(hmodule) => HINSTANCE(hmodule.0),
            Err(e) => {
                let _ = ready.send(Err(format!("GetModuleHandle failed: {e}")));
                return;
            }
        };
        ensure_window_class(hinstance);

        let hwnd = match CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            CLASS_NAME,
            w!("LeoPasteTx"),
            WINDOW_STYLE::default(),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            Some(hinstance),
            None,
        ) {
            Ok(hwnd) => hwnd,
            Err(e) => {
                let _ = ready.send(Err(format!("CreateWindowEx failed: {e}")));
                return;
            }
        };
        shared
            .owner
            .store(hwnd.0 as usize, std::sync::atomic::Ordering::Release);
        SetWindowLongPtrW(
            hwnd,
            GWLP_USERDATA,
            Arc::into_raw(shared.clone()) as *const _ as isize,
        );

        let published = match snapshot_clipboard(hwnd, &shared) {
            Ok(())
                if (shared.current)()
                    && !shared
                        .state
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .cancelled =>
            {
                match publish(hwnd) {
                    Ok(sequence) => Ok(sequence),
                    Err(e) => {
                        *shared.sequence.lock().unwrap_or_else(|e| e.into_inner()) =
                            GetClipboardSequenceNumber();
                        restore_snapshot(&shared);
                        Err(e)
                    }
                }
            }
            Ok(()) => Err("Dictation cancelled.".into()),
            Err(e) => Err(e),
        };
        let sequence = match published {
            Ok(sequence) => sequence,
            Err(e) => {
                destroy_window_and_shared(hwnd);
                let _ = ready.send(Err(e));
                return;
            }
        };
        *shared.sequence.lock().unwrap_or_else(|e| e.into_inner()) = sequence;
        shared
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .published_at = Instant::now();
        if let Ok(mut slot) = PENDING.lock() {
            *slot = Some(shared.clone());
        }
        let _ = SetTimer(Some(hwnd), TIMER_ID, TIMER_INTERVAL_MS, None);
        let _ = ready.send(Ok(()));

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = DispatchMessageW(&msg);
        }

        let _ = KillTimer(Some(hwnd), TIMER_ID);
        destroy_window_and_shared(hwnd);
    }
}

pub(super) fn run(
    text: &str,
    _app_handle: &tauri::AppHandle,
    enigo: &mut enigo::Enigo,
    current: Arc<dyn Fn() -> bool + Send + Sync>,
) -> Result<super::Read, String> {
    let shared = Arc::new(WinTxShared {
        state: Mutex::new(TxState::new()),
        text: text.to_string(),
        _activity: leo_core::activity::global()
            .begin()
            .map_err(|e| e.to_string())?,
        current,
        snapshot: Mutex::new(Vec::new()),
        sequence: Mutex::new(0),
        owner: std::sync::atomic::AtomicUsize::new(0),
        own: std::sync::atomic::AtomicBool::new(false),
    });

    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let shared_for_pump = shared.clone();
    thread::spawn(move || pump_thread(shared_for_pump, ready_tx));

    match ready_rx.recv_timeout(std::time::Duration::from_secs(5)) {
        Ok(Ok(())) => {}
        Ok(Err(e)) => return Err(e),
        Err(_) => {
            shared
                .state
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .cancelled = true;
            return Err(
                "Automatic paste took too long. Copy your text from Recent dictations.".into(),
            );
        }
    }
    info!("[reliable-paste] published transcript (delayed render)");

    shared
        .state
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .injected_at = Some(Instant::now());
    send_chord(enigo, &shared.state)?;
    Ok(Box::new(move || {
        shared
            .state
            .lock()
            .map(|st| st.any_receipt_after_injection())
            .unwrap_or(false)
    }))
}

#[cfg(test)]
mod tests {
    #[test]
    fn private_clipboard_formats_are_skipped() {
        assert!(super::preserved_format(0x0001));
        assert!(!super::preserved_format(0x0200));
        assert!(!super::preserved_format(0x02FF));
        assert!(super::preserved_format(0x0300));
        assert!(super::preserved_format(0xC000));
    }
}
