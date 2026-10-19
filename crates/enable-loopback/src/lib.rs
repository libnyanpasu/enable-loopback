//! Select language and dispatch CLI or GUI startup (§1.2/§3.1), checking runtime only for GUI.

mod args;
mod cli;
mod i18n;
pub mod model;
pub mod netiso;
mod runtime_check;
mod ui;

// rust-i18n's public macros resolve these hooks at the crate root; setup lives in i18n.
pub(crate) use i18n::{_rust_i18n_t, _rust_i18n_try_translate};
use windows::{
    Win32::{debugapi::OutputDebugStringW, winuser::*},
    core::HSTRING,
};

pub fn run() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let system = sys_locale::get_locale();
    let language = i18n::choose(args::language_hint(&arguments), system.as_deref());
    rust_i18n::set_locale(language);
    let parsed = match args::parse(&arguments) {
        Ok(parsed) => parsed,
        Err(error) => {
            let text = match error {
                args::Error::ParseName { .. } => rust_i18n::t!("cli.missing_name"),
                args::Error::ParseLanguage { .. } => rust_i18n::t!("cli.missing_language"),
            };
            show_message(&text, MB_ICONERROR as u32);
            std::process::exit(cli::FAILURE);
        }
    };
    let code = match parsed.mode {
        args::Mode::Cli(command) => match cli::run(command) {
            Ok(code) => code,
            Err(cli::Error::ExecuteNotImplemented { command }) => {
                debug(&rust_i18n::t!(
                    "cli.not_implemented",
                    command = format!("{command:?}")
                ));
                cli::FAILURE
            }
        },
        args::Mode::Gui(options) => match runtime_check::ensure_available() {
            Ok(true) => match ui::run(options) {
                Ok(()) => cli::SUCCESS,
                Err(error) => {
                    show_message(
                        &rust_i18n::t!("window.start_failed", detail = error.to_string()),
                        MB_ICONERROR as u32,
                    );
                    cli::FAILURE
                }
            },
            Ok(false) => cli::FAILURE,
            Err(error) => {
                debug(&error.to_string());
                cli::FAILURE
            }
        },
    };
    std::process::exit(code);
}

fn show_message(text: &str, flags: u32) {
    let text = HSTRING::from(text);
    let title = HSTRING::from(rust_i18n::t!("window.title").as_ref());
    // SAFETY: owned HSTRINGs remain live throughout this synchronous call.
    unsafe {
        MessageBoxW(None, &text, &title, flags);
    }
}

fn debug(text: &str) {
    let text = HSTRING::from(text);
    // SAFETY: text remains live for the synchronous debug-output call.
    unsafe {
        OutputDebugStringW(&text);
    }
}
