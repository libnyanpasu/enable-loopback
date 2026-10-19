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

/// All four flags are shared only by UI-thread code. Rc makes accidental Send impossible.
#[derive(Clone, Default)]
pub(super) struct CloseState {
    pub dirty: std::rc::Rc<std::cell::Cell<bool>>,
    pub saving: std::rc::Rc<std::cell::Cell<bool>>,
    pub confirmed: std::rc::Rc<std::cell::Cell<bool>>,
    pub requested: std::rc::Rc<std::cell::Cell<bool>>,
}

struct CloseSubclass {
    state: CloseState,
    sender: windows_reactor::LocalSender<super::Message>,
}
const CLOSE_SUBCLASS_ID: usize = 1;

pub(super) fn install_close(
    window: &windows_reactor::WindowHandle<'_>,
    state: CloseState,
    sender: windows_reactor::LocalSender<super::Message>,
) -> Result<(), Error> {
    use windows::Win32::commctrl::SetWindowSubclass;
    let data = Box::into_raw(Box::new(CloseSubclass { state, sender }));
    // SAFETY: run_window provides a live HWND on its UI thread. The box contains only
    // UI-thread state and is owned by this subclass until WM_NCDESTROY. It is never global.
    if !unsafe {
        SetWindowSubclass(
            window.as_raw().cast(),
            Some(close_proc),
            CLOSE_SUBCLASS_ID,
            data as usize,
        )
    }
    .as_bool()
    {
        let error = failure("SetWindowSubclass");
        // SAFETY: installation failed, so native code never took ownership of the box.
        unsafe {
            drop(Box::from_raw(data));
        }
        return Err(error);
    }
    Ok(())
}

unsafe extern "system" fn close_proc(
    hwnd: windows::Win32::HWND,
    message: u32,
    wparam: usize,
    lparam: isize,
    id: usize,
    data: usize,
) -> isize {
    use windows::Win32::{
        commctrl::{DefSubclassProc, RemoveWindowSubclass},
        winuser::{WM_CLOSE, WM_NCDESTROY},
    };
    if message == WM_NCDESTROY as u32 {
        // SAFETY: data is our unique live box. Remove the callback before releasing it;
        // subsequent destruction messages cannot access the freed UI-thread Rc values.
        unsafe {
            let _ = RemoveWindowSubclass(hwnd, Some(close_proc), id);
            drop(Box::from_raw(data as *mut CloseSubclass));
            return DefSubclassProc(hwnd, message, wparam, lparam);
        }
    }
    // SAFETY: successful installation retains this box until WM_NCDESTROY on this thread.
    let subclass = unsafe { &*(data as *const CloseSubclass) };
    if message == WM_CLOSE as u32 && !subclass.state.confirmed.get() {
        if subclass.state.saving.get() {
            return 0;
        }
        if subclass.state.dirty.get() {
            if !subclass.state.requested.replace(true)
                && !subclass.sender.send(super::Message::CloseRequested)
            {
                subclass.state.requested.set(false);
            }
            return 0;
        }
    }
    // SAFETY: all unconsumed messages retain their original parameters and subclass chain.
    unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
}

pub(super) fn version() -> Result<String, Error> {
    use windows::Win32::{sysinfoapi::GetVersionExW, winnt::OSVERSIONINFOW};
    let mut version = OSVERSIONINFOW {
        dwOSVersionInfoSize: size_of::<OSVERSIONINFOW>() as u32,
        ..Default::default()
    };
    // SAFETY: writable version structure has its required size field set. The executable's
    // supportedOS Windows 10 manifest enables the real Windows 10/11 version and build.
    if !unsafe { GetVersionExW(&mut version) }.as_bool() {
        return Err(failure("GetVersionExW"));
    }
    Ok(format!(
        "enable-loopback v{} {} Windows {}.{}.{}",
        env!("CARGO_PKG_VERSION"),
        if cfg!(target_arch = "aarch64") {
            "arm64"
        } else {
            "64bit"
        },
        version.dwMajorVersion,
        version.dwMinorVersion,
        version.dwBuildNumber
    ))
}

pub(super) fn open_help(hwnd: usize, url: &str) -> Result<(), Error> {
    use windows::{
        Win32::{
            shellapi::{SE_ERR_DLLNOTFOUND, ShellExecuteW},
            winuser::SW_SHOWNORMAL,
        },
        core::HSTRING,
    };
    let action = HSTRING::from("open");
    let url = HSTRING::from(url);
    // SAFETY: live UI-thread HWND, terminated verb and fixed URL, no parameters/directory.
    let result = unsafe {
        ShellExecuteW(
            Some(hwnd as _),
            &action,
            &url,
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    } as isize;
    // ShellExecuteW's documented error range ends at SE_ERR_DLLNOTFOUND (shellapi.h).
    if result <= SE_ERR_DLLNOTFOUND as isize {
        return Err(Error::OpenHelp {
            code: result as usize,
        });
    }
    Ok(())
}

/// Reveal a virtualized native ListView item. Reactor has no ListView reference or
/// ScrollIntoView API at a29e064. UIA runs in an MTA worker, never on our UI thread:
/// https://learn.microsoft.com/windows/win32/winauto/uiauto-threading
/// No COM interface or allocation escapes this worker's apartment.
pub(super) fn reveal(
    hwnd: usize,
    index: usize,
    token: &windows_reactor::CancellationToken,
) -> Result<(), Error> {
    use std::mem::ManuallyDrop;
    use windows::{
        Win32::{
            combaseapi::{CoCreateInstance, CoInitializeEx, CoUninitialize},
            oaidl::{VARIANT, VARIANT_0, VARIANT_0_0, VARIANT_0_0_0},
            objbase::COINIT_MULTITHREADED,
            oleauto::VariantClear,
            uiautomationclient::{
                CUIAutomation, IUIAutomation, IUIAutomationElement,
                IUIAutomationItemContainerPattern, IUIAutomationScrollItemPattern,
                IUIAutomationVirtualizedItemPattern, TreeScope_Descendants,
            },
            wtypes::VT_BSTR,
            wtypesbase::CLSCTX_INPROC_SERVER,
        },
        core::BSTR,
    };
    // UIAutomationClient.h identifiers; the pinned windows bindings omit these constants.
    const AUTOMATION_ID_PROPERTY: i32 = 30011;
    const ITEM_CONTAINER_PATTERN: i32 = 10019;
    const VIRTUALIZED_ITEM_PATTERN: i32 = 10020;
    const SCROLL_ITEM_PATTERN: i32 = 10017;
    // FindItemByProperty: propertyId zero requests the next item regardless of value.
    const NEXT_ITEM_PROPERTY: i32 = 0;
    struct Apartment;
    impl Drop for Apartment {
        fn drop(&mut self) {
            // SAFETY: this guard is created only after a successful CoInitializeEx.
            unsafe {
                CoUninitialize();
            }
        }
    }
    struct Variant(VARIANT);
    impl Drop for Variant {
        fn drop(&mut self) {
            // SAFETY: this guard owns the VT_BSTR allocation, freed exactly once.
            unsafe {
                let _ = VariantClear(&mut self.0);
            }
        }
    }
    let work = || -> windows::core::Result<()> {
        // SAFETY: reactor background workers do not own UI objects; initialize this call's
        // COM apartment and balance successful S_OK/S_FALSE returns on the same thread.
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED as u32) }.ok()?;
        let _apartment = Apartment;
        // SAFETY: documented UIA COM class, no aggregation, in-process activation.
        let automation: IUIAutomation =
            unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }?;
        let value = Variant(VARIANT {
            Anonymous: VARIANT_0 {
                Anonymous: ManuallyDrop::new(VARIANT_0_0 {
                    vt: VT_BSTR as u16,
                    Anonymous: VARIANT_0_0_0 {
                        bstrVal: ManuallyDrop::new(BSTR::from("apps")),
                    },
                    ..Default::default()
                }),
            },
        });
        // SAFETY: hwnd belongs to this window; UIA validates destroyed/invalid handles.
        // The property condition copies its VARIANT; the guard keeps the BSTR live here.
        let root = unsafe { automation.ElementFromHandle(hwnd as _) }?;
        let condition =
            unsafe { automation.CreatePropertyCondition(AUTOMATION_ID_PROPERTY, &value.0) }?;
        let list = unsafe { root.FindFirst(TreeScope_Descendants, &condition) }?;
        let items: IUIAutomationItemContainerPattern =
            unsafe { list.GetCurrentPatternAs(ITEM_CONTAINER_PATTERN) }?;
        let mut item: Option<IUIAutomationElement> = None;
        for _ in 0..=index {
            if token.is_cancelled() {
                return Ok(());
            }
            // SAFETY: UIA COM interfaces stay in this MTA; an empty VARIANT is ignored
            // when propertyId is zero. Each returned item is kept alive for the next call.
            item = Some(unsafe {
                items.FindItemByProperty(item.as_ref(), NEXT_ITEM_PROPERTY, &VARIANT::default())
            }?);
        }
        let item = item.expect("inclusive index range produces an item");
        if token.is_cancelled() {
            return Ok(());
        }
        // SAFETY: VirtualizedItem may be absent for an already realized container.
        if let Ok(virtualized) = unsafe {
            item.GetCurrentPatternAs::<IUIAutomationVirtualizedItemPattern>(
                VIRTUALIZED_ITEM_PATTERN,
            )
        } {
            unsafe { virtualized.Realize() }.ok()?;
        }
        if token.is_cancelled() {
            return Ok(());
        }
        // SAFETY: the realized item supplies ScrollItem; the UI thread remains free to
        // service the provider. Focus is requested via reactor after this worker completes.
        let scroll: IUIAutomationScrollItemPattern =
            unsafe { item.GetCurrentPatternAs(SCROLL_ITEM_PATTERN) }?;
        unsafe { scroll.ScrollIntoView() }.ok()
    };
    work().map_err(|source| Error::Native {
        operation: "ListView ScrollIntoView",
        source,
    })
}
