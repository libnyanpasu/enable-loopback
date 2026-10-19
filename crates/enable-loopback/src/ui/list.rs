//! Fixed-column presentation backed by WinUI's virtualizing ListView containers.
use super::{MainWindow, Message};
use crate::model::Column;
use windows_reactor::*;

pub(super) const COLUMNS: [(Column, &str); 7] = [
    (Column::DisplayName, "column.display_name"),
    (Column::Description, "column.description"),
    (Column::Package, "column.package"),
    (Column::Name, "column.name"),
    (Column::Sid, "column.sid"),
    (Column::Users, "column.users"),
    (Column::Binaries, "column.binaries"),
];

fn columns() -> [GridLength; 8] {
    [
        GridLength::Pixel(40.0),
        GridLength::Pixel(220.0),
        GridLength::Pixel(220.0),
        GridLength::Pixel(200.0),
        GridLength::Pixel(160.0),
        GridLength::Pixel(140.0),
        GridLength::Pixel(140.0),
        GridLength::STAR,
    ]
}

pub(super) fn view(window: &MainWindow, cx: &ViewContext<MainWindow>) -> View {
    let enabled = window.list.availability(false).list;
    let mut headers: Vec<View> = vec![Border::new().into()];
    for (column, key) in COLUMNS {
        let arrow = if window.list.sort.column == column {
            if window.list.sort.ascending {
                " ▲"
            } else {
                " ▼"
            }
        } else {
            ""
        };
        headers.push(
            Button::new()
                .grid_column(column as i32 + 1)
                .horizontal_alignment(HorizontalAlignment::Stretch)
                .style(ButtonStyle::Subtle)
                .is_enabled(enabled)
                .content(format!("{}{arrow}", rust_i18n::t!(key)))
                .on_click(cx.callback(move |()| Message::Sort(column)))
                .into(),
        );
    }
    Grid::new()
        .width(window.table_width)
        .rows([GridLength::Auto, GridLength::STAR])
        .children((
            // ListViewItem's standard content inset is 12 DIP on either side.
            Grid::new()
                .margin(Thickness::xy(12.0, 0.0))
                .columns(columns())
                .children(headers),
            ListView::new()
                .grid_row(1)
                .automation_id("apps")
                .is_enabled(enabled)
                .selection_mode(ListViewSelectionMode::Single)
                .items(window.list.rows.iter().map(|row| {
                    let sid = row.app.sid.clone();
                    let mut cells: Vec<View> =
                        vec![
                            CheckBox::new()
                                .width(40.0)
                                .min_width(0.0)
                                .is_checked(row.checked)
                                .automation_name(rust_i18n::t!(
                                    "column.exempt",
                                    name = row.columns(&window.placeholders)[0].clone()
                                ))
                                .on_is_checked_changed(cx.callback(move |v| {
                                    Message::Checked(sid.clone(), v == Some(true))
                                }))
                                .into(),
                        ];
                    for (index, value) in row.columns(&window.placeholders).iter().enumerate() {
                        cells.push(
                            TextBlock::new()
                                .grid_column(index as i32 + 1)
                                .text(value)
                                .max_lines(1)
                                .text_trimming(TextTrimming::CharacterEllipsis)
                                .vertical_alignment(VerticalAlignment::Center)
                                .margin(Thickness::xy(8.0, 0.0))
                                .tooltip(value),
                        );
                    }
                    let background: Brush = if !window.current_user.is_empty()
                        && row.highlights_user(&window.current_user)
                    {
                        ThemeBrush::CardStroke.into()
                    } else {
                        Color::transparent().into()
                    };
                    DataItem::new(
                        row.app.sid.to_string(),
                        row.columns(&window.placeholders)[0].clone(),
                    )
                    .content(
                        Border::new().background(background).content(
                            Grid::new()
                                .width(window.table_width - 48.0)
                                .columns(columns())
                                .children(cells),
                        ),
                    )
                })),
        ))
        .into()
}
