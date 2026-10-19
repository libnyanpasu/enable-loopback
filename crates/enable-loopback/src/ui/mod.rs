//! Reactor presentation and asynchronous NetIso wiring (§3/§4).
mod dialogs;
mod keys;
mod list;
mod native;

use crate::{
    args::GuiOptions,
    instance::WindowPublisher,
    model::{
        Change, Column, Direction, ListState, Modifiers, Placeholders, PointerButton, Selection,
        Status,
    },
    netiso::{self, AppContainer, Sid},
};
use dialogs::Dialog;
use snafu::Snafu;
use std::{
    cell::Cell,
    collections::{BTreeMap, VecDeque},
    rc::Rc,
    time::Duration,
};
use windows::{
    Win32::{
        libloaderapi::GetModuleHandleW,
        winerror::{EPT_S_NOT_REGISTERED, ERROR_SUCCESS},
        winuser::*,
    },
    core::PCWSTR,
};
use windows_reactor::*;

const LEARN_URL: &str =
    "https://learn.microsoft.com/previous-versions/windows/apps/hh780593(v=win.10)";

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("Publishing the window for other instances failed: {source}"))]
    PublishWindow { source: crate::instance::Error },
    #[snafu(display("Starting the reactor window failed: {source}"))]
    StartWindow { source: windows::core::Error },
    #[snafu(display("Loading icon resource {resource_id} at {size}px failed: {source}"))]
    LoadWindowIcon {
        resource_id: u16,
        size: i32,
        source: windows::core::Error,
    },
    #[snafu(display("{operation} failed: {source}"))]
    Native {
        operation: &'static str,
        source: windows::core::Error,
    },
}

struct MainWindow {
    list: ListState,
    options: GuiOptions,
    placeholders: Placeholders,
    current_user: String,
    status: String,
    dialog: Option<Dialog>,
    dialog_open: bool,
    dialogs: VecDeque<Dialog>,
    keep_prompting: bool,
    dialog_timer: Option<ComponentTimer>,
    table_width: f64,
    list_focus: ElementRef<Border>,
    selection: Selection,
    row_focus: BTreeMap<Sid, ElementRef<Border>>,
    hwnd: usize,
    checkbox_focus: Rc<Cell<bool>>,
}

#[derive(Clone)]
pub struct Startup {
    options: GuiOptions,
    publisher: Rc<WindowPublisher>,
}
impl PartialEq for Startup {
    fn eq(&self, other: &Self) -> bool {
        self.options == other.options && Rc::ptr_eq(&self.publisher, &other.publisher)
    }
}

struct Loaded {
    apps: Vec<AppContainer>,
    exempted: Vec<Sid>,
    user: String,
}
enum Message {
    WindowPrepared(Result<usize, Error>),
    Refresh,
    BeginRefresh(bool),
    Loaded {
        registry: bool,
        force: bool,
        fallback_code: Option<u32>,
        result: Result<Loaded, netiso::Error>,
    },
    Sort(Column),
    Checked(Sid, bool),
    SetAll(bool),
    DialogClosed(ContentDialogResult),
    NextDialog,
    KeepPrompting(bool),
    Size(WindowSize),
    Pointer(Sid, Column, PointerEventInfo),
    Focus(Option<usize>),
    ListKey(KeyEventInfo),
    Copy(bool),
    Save,
    Saved(Result<(), netiso::Error>),
}

fn load(registry: bool, force: bool) -> Result<Loaded, netiso::Error> {
    let apps = if registry {
        netiso::enumerate_from_registry()?
    } else {
        netiso::enumerate(force)?
    };
    if apps.is_empty() && !registry {
        return Err(netiso::Error::Enumerate {
            force_binaries: force,
            code: ERROR_SUCCESS as u32,
        });
    }
    Ok(Loaded {
        apps,
        exempted: netiso::exempted_sids()?,
        user: netiso::current_user()?,
    })
}

impl MainWindow {
    fn order(&self) -> Vec<Sid> {
        self.list.rows.iter().map(|r| r.app.sid.clone()).collect()
    }
    fn focus_row(&self) {
        if let Some(reference) = self
            .selection
            .focused
            .as_ref()
            .and_then(|sid| self.row_focus.get(sid))
        {
            let _ = reference.request_focus();
        }
    }
    fn copy(&mut self, column: bool) {
        let value = if column {
            crate::model::copy_column(
                &self.list.rows,
                &self.selection.selected,
                self.selection.context_column.unwrap_or(Column::DisplayName),
                &self.placeholders,
            )
        } else {
            crate::model::copy_rows(
                &self.list.rows,
                &self.selection.selected,
                &self.placeholders,
            )
        };
        if !value.is_empty()
            && let Err(error) = native::clipboard(self.hwnd, &value)
        {
            self.enqueue(Dialog::Error {
                title: rust_i18n::t!("dialog.error_title").into_owned(),
                body: error.to_string(),
            });
        }
    }
    fn enqueue(&mut self, dialog: Dialog) {
        self.dialogs.push_back(dialog);
        if self.dialog.is_none() {
            self.next_dialog();
        }
    }
    fn next_dialog(&mut self) {
        self.dialog = self.dialogs.pop_front();
        self.dialog_open = self.dialog.is_some();
        if !self.dialog_open && self.list.loaded {
            let _ = self.list_focus.request_focus();
        }
    }
    fn start_load(
        &self,
        registry: bool,
        force: bool,
        fallback_code: Option<u32>,
        cx: &ComponentContext<Self>,
    ) {
        cx.spawn_background(move |_| Message::Loaded {
            registry,
            force,
            fallback_code,
            result: load(registry, force),
        });
    }
    fn refresh(&mut self, force: bool, cx: &ComponentContext<Self>) {
        self.list.begin_refresh();
        self.status = rust_i18n::t!("status.refreshing").into_owned();
        self.start_load(self.options.registry, force, None, cx);
    }
    fn change_status(&mut self, status: Status) {
        self.status = status
            .key
            .map(|key| rust_i18n::t!(key).into_owned())
            .unwrap_or_default();
        if status.orphan_removal_count > 0 {
            self.status.push(' ');
            self.status.push_str(&rust_i18n::t!(
                "status.orphan_removal",
                count = status.orphan_removal_count
            ));
        }
    }
    fn load_failed(&mut self, error: netiso::Error, fallback_code: Option<u32>) {
        self.list.refreshing = false;
        let detail = error.to_string();
        let code = match error {
            netiso::Error::Enumerate { code, .. } | netiso::Error::ReadExemptions { code } => {
                Some(code)
            }
            _ => fallback_code,
        };
        let (title, body) = if let Some(code) = code {
            self.status =
                rust_i18n::t!("status.load_failed", code = format!("0x{code:x}")).into_owned();
            (
                rust_i18n::t!(if code == EPT_S_NOT_REGISTERED as u32 {
                    "dialog.firewall_title"
                } else {
                    "dialog.error_title"
                })
                .into_owned(),
                rust_i18n::t!(
                    if code == EPT_S_NOT_REGISTERED as u32 {
                        "dialog.firewall_body"
                    } else {
                        "dialog.load_failed"
                    },
                    detail = detail
                )
                .into_owned(),
            )
        } else {
            self.status = rust_i18n::t!("status.failed", detail = detail.clone()).into_owned();
            (rust_i18n::t!("dialog.error_title").into_owned(), detail)
        };
        self.enqueue(Dialog::Error { title, body });
    }
}

impl Component for MainWindow {
    type Message = Message;
    type Input = Startup;
    fn create(input: &Startup, cx: &ComponentContext<Self>) -> Self {
        let publisher = input.publisher.clone();
        if !cx.run_window(move |window| {
            // SAFETY: run_window supplies this process's live HWND on the UI thread.
            let hwnd = window.as_raw() as usize;
            let result = unsafe { publisher.publish_window(window.as_raw().cast()) }
                .map_err(|source| Error::PublishWindow { source })
                .and_then(|()| set_window_icons(window))
                .map(|()| hwnd);
            Message::WindowPrepared(result)
        }) {
            crate::debug("Native window callback could not be scheduled");
        }
        if !input.options.manual {
            let _ = cx.sender().send(Message::BeginRefresh(false));
        }
        Self {
            list: ListState::default(),
            options: input.options,
            placeholders: Placeholders::for_locale(&rust_i18n::locale()),
            current_user: String::new(),
            status: rust_i18n::t!("status.ready").into_owned(),
            dialog: None,
            dialog_open: false,
            dialogs: VecDeque::new(),
            keep_prompting: true,
            dialog_timer: None,
            table_width: 1240.0,
            list_focus: ElementRef::new(),
            selection: Selection::default(),
            row_focus: BTreeMap::new(),
            hwnd: 0,
            checkbox_focus: Rc::new(Cell::new(false)),
        }
    }
    fn update(&mut self, message: Message, cx: &ComponentContext<Self>) {
        match message {
            Message::WindowPrepared(Err(error)) => crate::debug(&error.to_string()),
            Message::WindowPrepared(Ok(hwnd)) => self.hwnd = hwnd,
            Message::BeginRefresh(force) => self.refresh(force, cx),
            Message::Refresh if self.list.availability().refresh && !self.dialog_open => {
                if self.list.is_dirty() {
                    self.enqueue(Dialog::Refresh);
                } else {
                    self.refresh(true, cx);
                }
            }
            Message::Loaded {
                registry,
                force,
                fallback_code,
                result,
            } => match result {
                Ok(loaded) => {
                    self.list
                        .replace_apps(loaded.apps, &loaded.exempted, &self.placeholders);
                    let order = self.order();
                    self.selection.prune(&order);
                    self.row_focus = order
                        .into_iter()
                        .map(|sid| (sid, ElementRef::new()))
                        .collect();
                    self.current_user = loaded.user;
                    match native::time() {
                        Ok(time) => {
                            self.status =
                                rust_i18n::t!("status.refreshed", time = time).into_owned()
                        }
                        Err(error) => {
                            crate::debug(&error.to_string());
                            self.status.clear();
                        }
                    }
                    if self.keep_prompting {
                        for sid in self.list.orphans.clone() {
                            self.enqueue(Dialog::Orphan {
                                sid: sid.to_string(),
                            });
                        }
                    }
                    let _ = self.list_focus.request_focus();
                }
                Err(netiso::Error::Enumerate { code, .. }) if !registry => {
                    self.enqueue(Dialog::Fallback { code, force })
                }
                Err(error) => self.load_failed(error, fallback_code),
            },
            Message::Sort(column) => {
                self.list.sort_by(column, &self.placeholders);
                let order = self.order();
                if let Some(index) = self.selection.first_selected_index(&order) {
                    self.selection.focused = Some(order[index].clone());
                    self.focus_row();
                }
            }
            Message::Pointer(sid, column, event) => {
                let button = if event.is_right_button_pressed {
                    PointerButton::Right
                } else if event.is_left_button_pressed {
                    PointerButton::Left
                } else {
                    return;
                };
                self.selection.click(
                    &self.order(),
                    &sid,
                    Modifiers {
                        control: event.modifiers.contains(InputModifiers::CONTROL),
                        shift: event.modifiers.contains(InputModifiers::SHIFT),
                    },
                    button,
                    column,
                );
                if button == PointerButton::Right {
                    match native::context_menu(self.hwnd) {
                        Ok(Some(command)) => {
                            let _ = cx.sender().send(match command {
                                native::MenuCommand::CopyRows => Message::Copy(false),
                                native::MenuCommand::CopyColumn => Message::Copy(true),
                                native::MenuCommand::ExemptAll => Message::SetAll(true),
                                native::MenuCommand::ExemptNone => Message::SetAll(false),
                            });
                        }
                        Ok(None) => {}
                        Err(error) => self.enqueue(Dialog::Error {
                            title: rust_i18n::t!("dialog.error_title").into_owned(),
                            body: error.to_string(),
                        }),
                    }
                }
            }
            Message::Focus(Some(index)) => {
                let order = self.order();
                if let Some(sid) = order.get(index) {
                    // Pointer/Shift selection has already updated focused before this event.
                    if self.selection.focused.as_ref() != Some(sid) {
                        self.selection.focus_changed(&order, sid);
                    }
                }
            }
            Message::ListKey(event) if self.list.availability().list => {
                let ctrl = event.modifiers.contains(InputModifiers::CONTROL);
                if ctrl && event.key == VirtualKey::A {
                    self.selection.select_all(&self.order());
                } else if ctrl && event.key == VirtualKey::C {
                    self.copy(false);
                } else if event.key == VirtualKey::UP || event.key == VirtualKey::DOWN {
                    self.selection.arrow(
                        &self.order(),
                        if event.key == VirtualKey::UP {
                            Direction::Up
                        } else {
                            Direction::Down
                        },
                        event.modifiers.contains(InputModifiers::SHIFT),
                    );
                    self.focus_row();
                } else if event.key == VirtualKey::SPACE
                    && let Some(sid) = &self.selection.focused
                {
                    if let Some(row) = self.list.rows.iter_mut().find(|r| r.app.sid == *sid) {
                        row.checked = !row.checked;
                    }
                    self.change_status(self.list.status_after_change(Change::Checked));
                }
            }
            Message::Copy(column) => self.copy(column),
            Message::Save if !self.dialog_open => {
                if let Some(sids) = self.list.begin_save() {
                    cx.spawn_background(move |_| Message::Saved(netiso::set_exempted(&sids)));
                }
            }
            Message::Saved(result) => {
                self.list.finish_save(result.is_ok());
                match result {
                    Ok(()) => self.status = rust_i18n::t!("status.saved").into_owned(),
                    Err(netiso::Error::SetExemptionsAccessDenied { .. }) => {
                        self.status = rust_i18n::t!("status.not_saved_admin").into_owned();
                        self.enqueue(Dialog::Error {
                            title: rust_i18n::t!("dialog.admin_title").into_owned(),
                            body: rust_i18n::t!("dialog.admin_body").into_owned(),
                        });
                    }
                    Err(error) => {
                        self.status = rust_i18n::t!("status.not_saved").into_owned();
                        self.enqueue(Dialog::Error {
                            title: rust_i18n::t!("dialog.save_title").into_owned(),
                            body: error.to_string(),
                        });
                    }
                }
            }
            Message::Checked(sid, checked) if self.list.availability().list => {
                if let Some(row) = self.list.rows.iter_mut().find(|row| row.app.sid == sid) {
                    row.checked = checked;
                }
                self.change_status(self.list.status_after_change(Change::Checked));
            }
            Message::SetAll(checked) if self.list.availability().bulk => {
                let status = self.list.set_all(checked);
                self.change_status(status);
            }
            Message::KeepPrompting(value) => self.keep_prompting = value,
            Message::DialogClosed(result) => {
                self.dialog_open = false;
                match &self.dialog {
                    Some(Dialog::Refresh) if result == ContentDialogResult::Primary => {
                        self.refresh(true, cx)
                    }
                    Some(Dialog::Fallback { code, force }) => {
                        self.start_load(true, *force, Some(*code), cx)
                    }
                    Some(Dialog::Orphan { .. }) if !self.keep_prompting => {
                        self.dialogs.retain(|d| !matches!(d, Dialog::Orphan { .. }))
                    }
                    _ => {}
                }
                // Publish a closed frame before opening another ContentDialog.
                self.dialog_timer =
                    Some(cx.set_local_timeout(Duration::from_millis(1), || Message::NextDialog));
            }
            Message::NextDialog => {
                self.dialog_timer = None;
                self.next_dialog();
            }
            Message::Size(size) => self.table_width = size.width.max(1240.0),
            _ => {}
        }
    }
    fn view(&self, _: &Startup, cx: &mut ViewContext<Self>) -> View {
        cx.window_title(rust_i18n::t!("window.title"));
        cx.window_visuals(
            WindowVisuals::new()
                .backdrop(WindowBackdrop::Mica)
                .client_size(960.0, 600.0)
                .constraints(WindowConstraints {
                    min_width: Some(640.0),
                    min_height: Some(400.0),
                    ..Default::default()
                }),
        );
        cx.on_window_size(cx.callback(Message::Size));
        let available = self.list.availability();
        let mut status: Vec<View> = Vec::new();
        if self.list.refreshing {
            status.push(
                ProgressRing::new()
                    .width(16.0)
                    .height(16.0)
                    .is_active(true)
                    .into(),
            );
        }
        status.push(
            TextBlock::new()
                .automation_id("status")
                .text(&self.status)
                .text_wrapping(TextWrapping::Wrap)
                .into(),
        );
        Grid::new()
            .rows([
                GridLength::Auto,
                GridLength::Auto,
                GridLength::STAR,
                GridLength::Auto,
            ])
            .children((
                explanation_bar()
                    .grid_row(0)
                    .margin(Thickness::new(16.0, 12.0, 16.0, 4.0)),
                Grid::new()
                    .grid_row(1)
                    .columns([GridLength::STAR, GridLength::Auto])
                    .children((
                        CommandBar::new().primary_commands((
                            AppBarButton::new()
                                .automation_id("refresh")
                                .label(rust_i18n::t!("toolbar.refresh"))
                                .icon(Symbol::Refresh)
                                .is_enabled(available.refresh)
                                .on_click(cx.callback(|()| Message::Refresh)),
                            AppBarSeparator::new(),
                            AppBarButton::new()
                                .label(rust_i18n::t!("toolbar.exempt_all"))
                                .icon(Symbol::SelectAll)
                                .is_enabled(available.bulk)
                                .on_click(cx.callback(|()| Message::SetAll(true))),
                            AppBarButton::new()
                                .label(rust_i18n::t!("toolbar.exempt_none"))
                                .icon(Symbol::ClearSelection)
                                .is_enabled(available.bulk)
                                .on_click(cx.callback(|()| Message::SetAll(false))),
                            AppBarSeparator::new(),
                            AppBarButton::new()
                                .automation_id("save")
                                .label(rust_i18n::t!("toolbar.save"))
                                .icon(Symbol::Save)
                                .is_enabled(available.save)
                                .on_click(cx.callback(|()| Message::Save)),
                        )),
                        TextBlock::new()
                            .grid_column(1)
                            .automation_id("counter")
                            .text(self.list.counter_text())
                            .vertical_alignment(VerticalAlignment::Center)
                            .margin(Thickness::xy(16.0, 0.0)),
                    )),
                Border::new()
                    .grid_row(2)
                    .element_ref(&self.list_focus)
                    .on_preview_key_down(keys::list(cx.sender(), self.checkbox_focus.clone()))
                    .is_tab_stop(true)
                    .content(
                        ScrollViewer::new()
                            .horizontal_scroll_bar_visibility(ScrollBarVisibility::Auto)
                            .vertical_scroll_bar_visibility(ScrollBarVisibility::Disabled)
                            .content(list::view(self, cx)),
                    ),
                Border::new()
                    .grid_row(3)
                    .padding(Thickness::xy(16.0, 8.0))
                    .content(
                        StackPanel::new()
                            .orientation(Orientation::Horizontal)
                            .spacing(8.0)
                            .children(status),
                    ),
            ))
            .content_dialog(dialogs::view(
                self.dialog.as_ref(),
                self.dialog_open,
                self.keep_prompting,
                cx,
            ))
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

pub fn run(options: GuiOptions, publisher: Rc<WindowPublisher>) -> Result<(), Error> {
    App::run_component::<MainWindow>(Startup { options, publisher })
        .map_err(|source| Error::StartWindow { source })
}
