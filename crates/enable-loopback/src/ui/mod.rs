//! Reactor window and controls (§4); later stages connect list state and background NetIso tasks.

use crate::{args::GuiOptions, model::ListState};
use snafu::Snafu;
use windows::{
    Win32::{libloaderapi::GetModuleHandleW, winuser::*},
    core::PCWSTR,
};
use windows_reactor::*;

const LEARN_URL: &str =
    "https://learn.microsoft.com/previous-versions/windows/apps/hh780593(v=win.10)";

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("Starting the reactor window failed: {source}"))]
    StartWindow { source: windows::core::Error },
    #[snafu(display("Loading icon resource {resource_id} at {size}px failed: {source}"))]
    LoadWindowIcon {
        resource_id: u16,
        size: i32,
        source: windows::core::Error,
    },
}

struct MainWindow {
    list: ListState,
    _options: GuiOptions,
}

enum Message {
    IconLoaded(Result<(), Error>),
}

impl Component for MainWindow {
    type Message = Message;
    type Input = GuiOptions;

    fn create(input: &GuiOptions, context: &ComponentContext<Self>) -> Self {
        if !context.run_window(|window| Message::IconLoaded(set_window_icons(window))) {
            crate::debug("Window icon callback could not be scheduled");
        }
        Self {
            list: ListState::default(),
            _options: *input,
        }
    }

    fn update(&mut self, message: Message, _: &ComponentContext<Self>) {
        if let Message::IconLoaded(Err(error)) = message {
            crate::debug(&error.to_string());
        }
    }

    fn view(&self, _: &GuiOptions, context: &mut ViewContext<Self>) -> View {
        context.window_title(rust_i18n::t!("window.title"));
        context.window_visuals(
            WindowVisuals::new()
                .backdrop(WindowBackdrop::Mica)
                .client_size(960.0, 600.0)
                .constraints(WindowConstraints {
                    min_width: Some(640.0),
                    min_height: Some(400.0),
                    ..Default::default()
                }),
        );
        Grid::new()
            .rows([
                GridLength::Auto,
                GridLength::Auto,
                GridLength::STAR,
                GridLength::Auto,
            ])
            .children((
                // Same 16px side gutter as the status bar.
                explanation_bar()
                    .grid_row(0)
                    .margin(Thickness::new(16.0, 12.0, 16.0, 4.0)),
                CommandBar::new().grid_row(1).primary_commands((
                    AppBarButton::new()
                        .label(rust_i18n::t!("toolbar.refresh"))
                        .icon(Symbol::Refresh)
                        .is_enabled(false),
                    AppBarSeparator::new(),
                    AppBarButton::new()
                        .label(rust_i18n::t!("toolbar.exempt_all"))
                        .icon(Symbol::SelectAll)
                        .is_enabled(false),
                    AppBarButton::new()
                        .label(rust_i18n::t!("toolbar.exempt_none"))
                        .icon(Symbol::ClearSelection)
                        .is_enabled(false),
                    AppBarSeparator::new(),
                    AppBarButton::new()
                        .label(rust_i18n::t!("toolbar.save"))
                        .icon(Symbol::Save)
                        .is_enabled(self.list.is_dirty()),
                )),
                ListView::new().grid_row(2).is_enabled(false),
                Border::new()
                    .grid_row(3)
                    .padding(Thickness::xy(16.0, 8.0))
                    .content(TextBlock::new().text(rust_i18n::t!("status.ready"))),
            ))
            .into()
    }
}

/// Informational InfoBar look-alike (§4.2). InfoBar has no Content/ActionButton relation in
/// a29e064, so the Learn more link would otherwise sit on a row of its own.
fn explanation_bar() -> Border {
    // Glyphs and sizes follow the WinUI InfoBar template: a status circle behind an "i".
    const ICON_BACKGROUND_GLYPH: &str = "\u{F136}";
    const INFORMATIONAL_ICON_GLYPH: &str = "\u{F13F}";
    const ICON_SIZE: f64 = 16.0;
    Border::new()
        .background(ThemeBrush::CardBackground)
        .border_brush(ThemeBrush::CardStroke)
        .border_thickness(Thickness::uniform(1.0))
        .corner_radius(CornerRadius::uniform(4.0))
        .padding(Thickness::new(16.0, 6.0, 8.0, 6.0))
        .content(
            Grid::new()
                .columns([GridLength::Auto, GridLength::STAR, GridLength::Auto])
                .children((
                    FontIcon::new()
                        .glyph(ICON_BACKGROUND_GLYPH)
                        .font_size(ICON_SIZE)
                        .foreground(ThemeBrush::Accent),
                    // The window base fill contrasts with the accent in both themes.
                    FontIcon::new()
                        .glyph(INFORMATIONAL_ICON_GLYPH)
                        .font_size(ICON_SIZE)
                        .foreground(ThemeBrush::SolidBackground),
                    TextBlock::new()
                        .grid_column(1)
                        .margin(Thickness::new(14.0, 6.0, 12.0, 6.0))
                        .vertical_alignment(VerticalAlignment::Center)
                        .text_wrapping(TextWrapping::Wrap)
                        .text(rust_i18n::t!("window.explanation")),
                    HyperlinkButton::new()
                        .grid_column(2)
                        .vertical_alignment(VerticalAlignment::Center)
                        .content(rust_i18n::t!("window.learn_more").into_owned())
                        .navigate_uri(LEARN_URL)
                        .expect("fixed Microsoft Learn URI"),
                )),
        )
}

#[allow(
    clippy::manual_dangling_ptr,
    reason = "MAKEINTRESOURCEW(1) is an integer resource identifier"
)]
fn set_window_icons(window: WindowHandle<'_>) -> Result<(), Error> {
    // SAFETY: null requests our module; resource ID 1 is embedded by build.rs.
    // LR_SHARED ties icon lifetime to the module, so no icon handle needs freeing.
    let module = unsafe { GetModuleHandleW(PCWSTR::null()) };
    // SAFETY: run_window supplies a live HWND; use its current monitor DPI.
    let dpi = unsafe { GetDpiForWindow(window.as_raw().cast()) };
    for (kind, metric) in [(ICON_BIG, SM_CXICON), (ICON_SMALL, SM_CXSMICON)] {
        // SAFETY: both metrics are valid icon dimensions at this window DPI.
        let size = unsafe { GetSystemMetricsForDpi(metric, dpi) };
        let icon = unsafe {
            LoadImageW(
                Some(module),
                PCWSTR(1usize as *const u16),
                IMAGE_ICON as u32,
                size,
                size,
                LR_SHARED as u32,
            )
        };
        if icon.is_null() {
            return Err(Error::LoadWindowIcon {
                resource_id: 1,
                size,
                source: windows::core::Error::from_thread(),
            });
        }
        // SAFETY: run_window supplies a live HWND on its UI thread; icon is module-owned.
        unsafe {
            SendMessageW(
                window.as_raw().cast(),
                WM_SETICON as u32,
                kind as usize,
                icon as isize,
            );
        }
    }
    Ok(())
}

pub fn run(options: GuiOptions) -> Result<(), Error> {
    App::run_component::<MainWindow>(options).map_err(|source| Error::StartWindow { source })
}
