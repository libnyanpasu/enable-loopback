//! Small native presentation helpers, kept outside the pure model.
use super::Error;
use windows::{
    Win32::datetimeapi::GetTimeFormatEx,
    core::{PCWSTR, PWSTR},
};

pub(super) fn time() -> Result<String, Error> {
    // SAFETY: null locale selects LOCALE_NAME_USER_DEFAULT, null time selects now,
    // and null format with flags zero selects the user's long time format.
    let length = unsafe { GetTimeFormatEx(PCWSTR::null(), 0, None, PCWSTR::null(), None, 0) };
    if length == 0 {
        return Err(failure("GetTimeFormatEx"));
    }
    let mut buffer = vec![0u16; length as usize];
    // SAFETY: buffer holds the character count returned by the sizing call.
    let written = unsafe {
        GetTimeFormatEx(
            PCWSTR::null(),
            0,
            None,
            PCWSTR::null(),
            Some(PWSTR(buffer.as_mut_ptr())),
            length,
        )
    };
    if written == 0 {
        return Err(failure("GetTimeFormatEx"));
    }
    Ok(String::from_utf16_lossy(&buffer[..written as usize - 1]))
}

pub(super) fn failure(operation: &'static str) -> Error {
    Error::Native {
        operation,
        source: windows::core::Error::from_thread(),
    }
}

/// Transfer an owned movable UTF-16 allocation to the clipboard only on success.
pub(super) fn clipboard(hwnd: usize, text: &str) -> Result<(), Error> {
    use windows::Win32::{
        HGLOBAL,
        winbase::{GMEM_MOVEABLE, GlobalAlloc, GlobalFree, GlobalLock, GlobalUnlock},
        winuser::{
            CF_UNICODETEXT, CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
        },
    };
    struct Allocation(HGLOBAL);
    impl Drop for Allocation {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: until SetClipboardData succeeds this guard owns the allocation.
                unsafe {
                    GlobalFree(self.0);
                }
            }
        }
    }
    struct Clipboard;
    impl Drop for Clipboard {
        fn drop(&mut self) {
            // SAFETY: constructed only after this UI thread opens the clipboard.
            unsafe {
                let _ = CloseClipboard();
            }
        }
    }
    let wide: Vec<u16> = text.encode_utf16().chain([0]).collect();
    // SAFETY: nonzero checked UTF-16 buffer size; movable memory is required by Win32.
    let mut allocation = Allocation(unsafe {
        GlobalAlloc(GMEM_MOVEABLE as u32, std::mem::size_of_val(wide.as_slice()))
    });
    if allocation.0.is_null() {
        return Err(failure("GlobalAlloc"));
    }
    // SAFETY: the guard owns live global memory and no other thread uses it.
    let pointer = unsafe { GlobalLock(allocation.0) }.cast::<u16>();
    if pointer.is_null() {
        return Err(failure("GlobalLock"));
    }
    // SAFETY: allocation has exactly wide.len() u16 elements; buffers do not overlap.
    unsafe {
        std::ptr::copy_nonoverlapping(wide.as_ptr(), pointer, wide.len());
        // Returning zero when the lock count reaches zero is normal.
        let _ = GlobalUnlock(allocation.0);
    }
    // SAFETY: hwnd was supplied by run_window and is live on this UI thread.
    if !unsafe { OpenClipboard(Some(hwnd as _)) }.as_bool() {
        return Err(failure("OpenClipboard"));
    }
    let _clipboard = Clipboard;
    // SAFETY: clipboard is held open by this thread; hwnd becomes its owner.
    if !unsafe { EmptyClipboard() }.as_bool() {
        return Err(failure("EmptyClipboard"));
    }
    // SAFETY: unlocked movable memory contains NUL-terminated UTF-16. Success transfers
    // ownership to Windows; failure leaves ownership with the allocation guard.
    if unsafe { SetClipboardData(CF_UNICODETEXT as u32, Some(allocation.0)) }.is_null() {
        return Err(failure("SetClipboardData"));
    }
    allocation.0 = std::ptr::null_mut();
    Ok(())
}

pub(super) enum MenuCommand {
    CopyRows,
    CopyColumn,
    ExemptNone,
    ExemptAll,
}

/// Reactor a29e064 rejects MenuExt on a Border (ir.rs:166–173).
pub(super) fn context_menu(hwnd: usize) -> Result<Option<MenuCommand>, Error> {
    use windows::{
        Win32::{HMENU, POINT, winuser::*},
        core::HSTRING,
    };
    const COPY_ROWS: usize = 1;
    const COPY_COLUMN: usize = 2;
    const EXEMPT_NONE: usize = 3;
    const EXEMPT_ALL: usize = 4;
    struct Menu(HMENU);
    impl Drop for Menu {
        fn drop(&mut self) {
            // SAFETY: this guard owns the popup and destroys it after tracking ends.
            unsafe {
                let _ = DestroyMenu(self.0);
            }
        }
    }
    // SAFETY: no arguments; the returned popup is immediately guarded.
    let menu = Menu(unsafe { CreatePopupMenu() });
    if menu.0.is_null() {
        return Err(failure("CreatePopupMenu"));
    }
    for (id, key) in [
        (COPY_ROWS, "menu.copy_rows"),
        (COPY_COLUMN, "menu.copy_column"),
        (0, ""),
        (EXEMPT_NONE, "toolbar.exempt_none"),
        (EXEMPT_ALL, "toolbar.exempt_all"),
    ] {
        let label = if id == 0 {
            HSTRING::new()
        } else {
            HSTRING::from(rust_i18n::t!(key).as_ref())
        };
        // SAFETY: live menu and terminated label; a separator does not use the string.
        if !unsafe {
            AppendMenuW(
                menu.0,
                if id == 0 { MF_SEPARATOR } else { MF_STRING } as u32,
                id,
                &label,
            )
        }
        .as_bool()
        {
            return Err(failure("AppendMenuW"));
        }
    }
    let mut point = POINT::default();
    // SAFETY: writable point receives screen coordinates at the right-click position.
    if !unsafe { GetCursorPos(&mut point) }.as_bool() {
        return Err(failure("GetCursorPos"));
    }
    // SAFETY: UI-thread-owned window/menu; RETURNCMD returns the ID without WM_COMMAND.
    // Zero means cancellation. Tracking runs the standard native menu message loop.
    let chosen = unsafe {
        TrackPopupMenuEx(
            menu.0,
            (TPM_RETURNCMD | TPM_RIGHTBUTTON) as u32,
            point.x,
            point.y,
            hwnd as _,
            None,
        )
    }
    .0 as usize;
    Ok(match chosen {
        COPY_ROWS => Some(MenuCommand::CopyRows),
        COPY_COLUMN => Some(MenuCommand::CopyColumn),
        EXEMPT_NONE => Some(MenuCommand::ExemptNone),
        EXEMPT_ALL => Some(MenuCommand::ExemptAll),
        _ => None,
    })
}
