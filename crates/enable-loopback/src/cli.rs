//! CLI dispatch, localized syntax, and legacy exit codes (§3.8), independent of WinAppSDK.

use crate::args::Command;
use snafu::Snafu;

pub const SUCCESS: i32 = 0;
pub const FAILURE: i32 = -1;
pub const SYNTAX: i32 = -10;

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("Command execution is not implemented: {command:?}"))]
    ExecuteNotImplemented { command: Command },
}

pub fn run(command: Command) -> Result<i32, Error> {
    if command == Command::Syntax {
        crate::show_message(
            &rust_i18n::t!("cli.syntax"),
            windows::Win32::winuser::MB_ICONINFORMATION as u32,
        );
        return Ok(SYNTAX);
    }
    Err(Error::ExecuteNotImplemented { command })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn commands_fail_explicitly_without_running_netiso_or_reactor() {
        for command in [
            Command::All,
            Command::None,
            Command::Allow { name: "*".into() },
            Command::Deny {
                name: "Store".into(),
            },
        ] {
            let Err(Error::ExecuteNotImplemented { command: actual }) = run(command.clone()) else {
                panic!("command unexpectedly executed");
            };
            assert_eq!(actual, command);
        }
    }
}
