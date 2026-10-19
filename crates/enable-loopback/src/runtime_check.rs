//! Check the framework runtime before reactor bootstrap; offer a localized download prompt (§8.2).

use snafu::Snafu;
use windows::{
    Win32::{
        appmodel::*,
        commctrl::*,
        shellapi::ShellExecuteW,
        winerror::ERROR_INSUFFICIENT_BUFFER,
        winnt::{PROCESSOR_ARCHITECTURE_AMD64, PROCESSOR_ARCHITECTURE_ARM64},
        winuser::*,
    },
    core::{HSTRING, PCWSTR, PWSTR, w},
};

pub const DOWNLOAD_URL: &str = "https://learn.microsoft.com/windows/apps/windows-app-sdk/downloads";
const FAMILY: PCWSTR = w!("Microsoft.WindowsAppRuntime.2_8wekyb3d8bbwe");
// windows-rs a29e064, crates/libs/reactor/src/native/bindings.rs:32212–32216,
// WINDOWSAPPSDK_RUNTIME_VERSION_{MAJOR,MINOR,BUILD,REVISION,UINT64}.
const REQUIRED_VERSION: [u16; 4] = [2, 5, 1, 0];
#[cfg(target_arch = "x86_64")]
const PROCESS_ARCHITECTURE: u32 = PROCESSOR_ARCHITECTURE_AMD64 as u32;
#[cfg(target_arch = "aarch64")]
const PROCESS_ARCHITECTURE: u32 = PROCESSOR_ARCHITECTURE_ARM64 as u32;
#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
compile_error!("enable-loopback supports Windows x64 and arm64 only");

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("GetPackagesByPackageFamily failed with status {status:#x}"))]
    EnumeratePackages { status: i32 },
    #[snafu(display(
        "PackageIdFromFullName failed for {package_full_name} with status {status:#x}"
    ))]
    ReadPackageId {
        package_full_name: String,
        status: i32,
    },
    #[snafu(display("ShellExecuteW failed for {url} with status {status}"))]
    OpenDownload { url: String, status: isize },
    #[snafu(display("Displaying the runtime download dialog failed: {source}"))]
    ShowDialog { source: windows::core::Error },
}

fn required_version_u64() -> u64 {
    REQUIRED_VERSION
        .into_iter()
        .fold(0, |version, part| (version << 16) | u64::from(part))
}

fn compatible(version: u64, architecture: u32) -> bool {
    version >= required_version_u64() && architecture == PROCESS_ARCHITECTURE
}

/// Query packages registered for the current user without COM or WinAppSDK initialization.
/// Retry if registration changes between the size query and the buffered query.
pub fn is_installed() -> Result<bool, Error> {
    for _ in 0..3 {
        let (mut count, mut length) = (0, 0);
        // SAFETY: output counters are valid; null buffers request required sizes.
        let status =
            unsafe { GetPackagesByPackageFamily(FAMILY, &mut count, None, &mut length, None) };
        if status == 0 && count == 0 {
            return Ok(false);
        }
        if status != ERROR_INSUFFICIENT_BUFFER {
            return Err(Error::EnumeratePackages { status });
        }
        let mut names = vec![PWSTR::null(); count as usize];
        let mut buffer = vec![0u16; length as usize];
        // SAFETY: both buffers have the exact capacities returned by the first query.
        let status = unsafe {
            GetPackagesByPackageFamily(
                FAMILY,
                &mut count,
                Some(names.as_mut_ptr()),
                &mut length,
                Some(buffer.as_mut_ptr()),
            )
        };
        if status == ERROR_INSUFFICIENT_BUFFER {
            continue;
        }
        if status != 0 {
            return Err(Error::EnumeratePackages { status });
        }
        for name in names.into_iter().take(count as usize) {
            // SAFETY: name is a terminated string pointing into the still-live buffer.
            if unsafe { package_is_compatible(PCWSTR(name.0)) }? {
                return Ok(true);
            }
        }
        return Ok(false);
    }
    Err(Error::EnumeratePackages {
        status: ERROR_INSUFFICIENT_BUFFER,
    })
}

/// `name` must point to a live, NUL-terminated package full name.
unsafe fn package_is_compatible(name: PCWSTR) -> Result<bool, Error> {
    let mut length = 0;
    // SAFETY: caller guarantees name validity; null buffer requests its size.
    let status = unsafe { PackageIdFromFullName(name, 0, &mut length, None) };
    if status != ERROR_INSUFFICIENT_BUFFER {
        return Err(Error::ReadPackageId {
            package_full_name: unsafe { name.to_string() }.unwrap_or_default(),
            status,
        });
    }
    // u64 storage supplies alignment as well as the requested byte count.
    let mut buffer = vec![0u64; (length as usize).div_ceil(size_of::<u64>())];
    // SAFETY: buffer is aligned and has at least length bytes; name is still live.
    let status =
        unsafe { PackageIdFromFullName(name, 0, &mut length, Some(buffer.as_mut_ptr().cast())) };
    if status != 0 {
        return Err(Error::ReadPackageId {
            package_full_name: unsafe { name.to_string() }.unwrap_or_default(),
            status,
        });
    }
    // SAFETY: successful API call populated PACKAGE_ID and its trailing strings.
    let id = unsafe { buffer.as_ptr().cast::<PACKAGE_ID>().read_unaligned() };
    // SAFETY: Version is the packed integer representation of the version union.
    Ok(compatible(
        unsafe { id.version.Anonymous.Version },
        id.processorArchitecture,
    ))
}

/// Build the official installer URL for a supported process architecture.
fn installer_url(architecture: u32) -> String {
    let architecture = match architecture as i32 {
        PROCESSOR_ARCHITECTURE_AMD64 => "x64",
        PROCESSOR_ARCHITECTURE_ARM64 => "arm64",
        _ => unreachable!("only x64 and arm64 processes are supported"),
    };
    let [major, minor, build, _] = REQUIRED_VERSION;
    format!(
        "https://aka.ms/windowsappsdk/{major}.{minor}/{major}.{minor}.{build}/windowsappruntimeinstall-{architecture}.exe"
    )
}

/// Returns false after a missing-runtime prompt. An inconclusive check lets reactor bootstrap.
pub fn ensure_available() -> Result<bool, Error> {
    match is_installed() {
        Ok(true) => return Ok(true),
        Ok(false) => {}
        Err(error) => {
            crate::debug(&error.to_string());
            return Ok(true);
        }
    }
    const DOWNLOAD_INSTALLER: i32 = 100;
    const OPEN_DOWNLOAD_PAGE: i32 = 101;
    let version = REQUIRED_VERSION.map(|part| part.to_string()).join(".");
    let text = HSTRING::from(
        rust_i18n::t!(
            "runtime.missing",
            version = version,
            architecture = std::env::consts::ARCH
        )
        .as_ref(),
    );
    let title = HSTRING::from(rust_i18n::t!("runtime.title").as_ref());
    let installer = HSTRING::from(rust_i18n::t!("runtime.download_installer").as_ref());
    let page = HSTRING::from(rust_i18n::t!("runtime.open_download_page").as_ref());
    let cancel = HSTRING::from(rust_i18n::t!("runtime.cancel").as_ref());
    let buttons = [
        TASKDIALOG_BUTTON {
            nButtonID: DOWNLOAD_INSTALLER,
            pszButtonText: PCWSTR(installer.as_ptr()),
        },
        TASKDIALOG_BUTTON {
            nButtonID: OPEN_DOWNLOAD_PAGE,
            pszButtonText: PCWSTR(page.as_ptr()),
        },
        TASKDIALOG_BUTTON {
            nButtonID: IDCANCEL,
            pszButtonText: PCWSTR(cancel.as_ptr()),
        },
    ];
    let config = TASKDIALOGCONFIG {
        cbSize: size_of::<TASKDIALOGCONFIG>() as u32,
        dwFlags: TDF_ALLOW_DIALOG_CANCELLATION | TDF_SIZE_TO_CONTENT,
        pszWindowTitle: PCWSTR(title.as_ptr()),
        pszContent: PCWSTR(text.as_ptr()),
        cButtons: buttons.len() as u32,
        pButtons: buttons.as_ptr(),
        nDefaultButton: IDCANCEL,
        ..Default::default()
    };
    let mut answer = IDCANCEL;
    // SAFETY: config, button array and all backing strings live through the modal call.
    unsafe { TaskDialogIndirect(&config, Some(&mut answer), None, None) }
        .ok()
        .map_err(|source| Error::ShowDialog { source })?;
    let url = match answer {
        DOWNLOAD_INSTALLER => installer_url(PROCESS_ARCHITECTURE),
        OPEN_DOWNLOAD_PAGE => DOWNLOAD_URL.to_owned(),
        _ => return Ok(false),
    };
    let target = HSTRING::from(&url);
    // SAFETY: URL is a derived official HTTPS address; no untrusted command or arguments.
    let result = unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            &target,
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    let status = result as isize;
    if status <= 32 {
        return Err(Error::OpenDownload { url, status });
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn installer_urls_match_required_version_and_both_architectures() {
        assert_eq!(
            installer_url(PROCESSOR_ARCHITECTURE_AMD64 as u32),
            "https://aka.ms/windowsappsdk/2.5/2.5.1/windowsappruntimeinstall-x64.exe"
        );
        assert_eq!(
            installer_url(PROCESSOR_ARCHITECTURE_ARM64 as u32),
            "https://aka.ms/windowsappsdk/2.5/2.5.1/windowsappruntimeinstall-arm64.exe"
        );
    }

    #[test]
    fn checks_all_version_components_and_process_architecture() {
        let required = required_version_u64();
        assert!(compatible(required, PROCESS_ARCHITECTURE));
        assert!(compatible(required + 1, PROCESS_ARCHITECTURE));
        assert!(!compatible(required - 1, PROCESS_ARCHITECTURE));
        assert!(!compatible(
            required,
            if PROCESS_ARCHITECTURE == PROCESSOR_ARCHITECTURE_AMD64 as u32 {
                PROCESSOR_ARCHITECTURE_ARM64 as u32
            } else {
                PROCESSOR_ARCHITECTURE_AMD64 as u32
            }
        ));
    }
}
