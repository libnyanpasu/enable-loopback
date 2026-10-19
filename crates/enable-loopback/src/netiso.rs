//! Own FirewallAPI data and expose UI-independent enumeration/configuration operations (§2).
//!
//! Stage 1 provides typed boundaries only. Later stages add FFI with RAII allocation guards,
//! indirect string resolution, SID/account conversion, and the registry fallback.

#![allow(
    dead_code,
    reason = "Stage-1 NetIso boundaries are wired up in later stages"
)]

use snafu::Snafu;

/// Owned, opaque SID bytes. No borrowed pointers escape the future FFI boundary.
/// Construction from Windows SIDs and structural validation are deferred to the FFI stage.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Sid(pub(crate) Vec<u8>);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppContainer {
    pub sid: Sid,
    pub users: Vec<String>,
    pub name: Option<String>,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub package_full_name: Option<String>,
    pub binaries: Vec<String>,
    pub exempt_at_last_check: bool,
}

#[derive(Debug, Snafu)]
#[allow(
    clippy::enum_variant_names,
    reason = "Identify the unimplemented stage-1 operations"
)]
pub enum Error {
    #[snafu(display(
        "AppContainer enumeration is unimplemented (force_binaries={force_binaries})"
    ))]
    EnumerateUnimplemented { force_binaries: bool },
    #[snafu(display("Reading loopback exemptions is unimplemented"))]
    ReadExemptionsUnimplemented {},
    #[snafu(display("Writing {sid_count} loopback exemptions is unimplemented"))]
    SetExemptionsUnimplemented { sid_count: usize },
}

pub fn enumerate(force_binaries: bool) -> Result<Vec<AppContainer>, Error> {
    Err(Error::EnumerateUnimplemented { force_binaries })
}

pub fn exempted_sids() -> Result<Vec<Sid>, Error> {
    Err(Error::ReadExemptionsUnimplemented {})
}

/// Replaces the entire exemption table, including orphan removal (design section 3.5).
pub fn set_exempted(sids: &[Sid]) -> Result<(), Error> {
    Err(Error::SetExemptionsUnimplemented {
        sid_count: sids.len(),
    })
}
