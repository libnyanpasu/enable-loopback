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
