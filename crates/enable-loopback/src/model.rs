//! Pure rows, rendering, sorting, copy formats, save baselines and CLI filters (§3).

mod selection;
pub use selection::{Direction, Modifiers, PointerButton, Selection};

use crate::{
    args::Command,
    netiso::{AppContainer, Sid},
};
use std::collections::BTreeSet;

#[derive(Clone, Debug)]
pub struct Row {
    pub app: AppContainer,
    pub checked: bool,
}

/// In display/copy order; the checkbox is separate and is never a sort column.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(usize)]
pub enum Column {
    #[default]
    DisplayName,
    Description,
    Package,
    Name,
    Sid,
    Users,
    Binaries,
}

#[derive(Clone, Debug)]
pub struct Placeholders {
    pub display_name: String,
    pub description: String,
    pub package: String,
    pub name: String,
    pub sid: String,
    pub users: String,
    pub binaries: String,
}

impl Placeholders {
    /// An explicit locale keeps rendering independent of the process's current locale.
    pub fn for_locale(locale: &str) -> Self {
        Self {
            display_name: rust_i18n::t!("placeholder.display_name", locale = locale).into_owned(),
            description: rust_i18n::t!("placeholder.description", locale = locale).into_owned(),
            package: rust_i18n::t!("placeholder.package", locale = locale).into_owned(),
            name: rust_i18n::t!("placeholder.name", locale = locale).into_owned(),
            sid: rust_i18n::t!("placeholder.sid", locale = locale).into_owned(),
            users: rust_i18n::t!("placeholder.users", locale = locale).into_owned(),
            binaries: rust_i18n::t!("placeholder.binaries", locale = locale).into_owned(),
        }
    }
}

impl Row {
    /// Render the values compared and copied by ListViewItemComparer/frmMain.
    /// Only null fields receive placeholders; present empty strings stay empty.
    pub fn columns(&self, placeholders: &Placeholders) -> [String; 7] {
        let app = &self.app;
        [
            app.display_name
                .as_ref()
                .unwrap_or(&placeholders.display_name)
                .clone(),
            app.description
                .as_ref()
                .unwrap_or(&placeholders.description)
                .clone(),
            app.package_full_name
                .as_ref()
                .unwrap_or(&placeholders.package)
                .clone(),
            app.name.as_ref().unwrap_or(&placeholders.name).clone(),
            app.sid.to_string(), // Sid is validated; missing AC SIDs cannot enter a Row.
            if app.users.is_empty() {
                placeholders.users.clone()
            } else {
                app.users.join("; ")
            },
            if app.binaries.is_empty() {
                placeholders.binaries.clone()
            } else {
                app.binaries.join("; ")
            },
        ]
    }

    /// Original highlighting tests the joined users for DOMAIN\user as a substring.
    pub fn highlights_user(&self, current_user: &str) -> bool {
        self.app.users.join("; ").contains(current_user)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sort {
    pub column: Column,
    pub ascending: bool,
}

impl Default for Sort {
    fn default() -> Self {
        Self {
            column: Column::DisplayName,
            ascending: true,
        }
    }
}

impl Sort {
    /// Like ListViewItemComparer, another column keeps the previous direction.
    pub fn click(&mut self, column: Column) {
        if column == self.column {
            self.ascending = !self.ascending;
        }
        self.column = column;
    }

    pub fn apply(&self, rows: &mut [Row], placeholders: &Placeholders) {
        // .NET Ordinal compares UTF-16 code units, not Rust's UTF-8 byte order.
        if self.ascending {
            rows.sort_by_cached_key(|row| {
                row.columns(placeholders)[self.column as usize]
                    .encode_utf16()
                    .collect::<Vec<_>>()
            });
        } else {
            rows.sort_by_cached_key(|row| {
                std::cmp::Reverse(
                    row.columns(placeholders)[self.column as usize]
                        .encode_utf16()
                        .collect::<Vec<_>>(),
                )
            });
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    Checked,
    ExemptAll,
    ExemptNone,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    /// None means clear the status bar after reverting all changes.
    pub key: Option<&'static str>,
    /// Append status.orphan_removal with this count when nonzero.
    pub orphan_removal_count: usize,
}

#[derive(Debug, Default)]
pub struct ListState {
    pub rows: Vec<Row>,
    pub orphans: Vec<Sid>,
    pub sort: Sort,
    /// Suppresses dirty notifications while refresh replaces rows (§3.3).
    pub refreshing: bool,
}

impl ListState {
    pub fn from_apps(
        apps: Vec<AppContainer>,
        exempted: &[Sid],
        placeholders: &Placeholders,
    ) -> Self {
        let mut rows: Vec<_> = apps
            .into_iter()
            .map(|mut app| {
                app.exempt_at_last_check = exempted.contains(&app.sid);
                Row {
                    checked: app.exempt_at_last_check,
                    app,
                }
            })
            .collect();
        // Exemption order is also the original orphan-prompt order.
        let orphans = exempted
            .iter()
            .filter(|sid| !rows.iter().any(|row| row.app.sid == **sid))
            .cloned()
            .collect();
        let sort = Sort::default();
        sort.apply(&mut rows, placeholders);
        Self {
            rows,
            orphans,
            sort,
            refreshing: false,
        }
    }

    pub fn is_dirty(&self) -> bool {
        !self.refreshing
            && self
                .rows
                .iter()
                .any(|row| row.checked != row.app.exempt_at_last_check)
    }

    /// Refresh retains the column and direction chosen before the refresh.
    pub fn replace_apps(
        &mut self,
        apps: Vec<AppContainer>,
        exempted: &[Sid],
        placeholders: &Placeholders,
    ) {
        let sort = self.sort;
        *self = Self::from_apps(apps, exempted, placeholders);
        self.sort = sort;
        self.sort.apply(&mut self.rows, placeholders);
    }

    pub fn status_after_change(&self, change: Change) -> Status {
        if !self.is_dirty() {
            return Status {
                key: None,
                orphan_removal_count: 0,
            };
        }
        Status {
            key: Some(match change {
                Change::Checked => "status.dirty",
                Change::ExemptAll => "status.exempt_all",
                Change::ExemptNone => "status.exempt_none",
            }),
            orphan_removal_count: self.orphans.len(),
        }
    }

    pub fn set_all(&mut self, checked: bool) -> Status {
        for row in &mut self.rows {
            row.checked = checked;
        }
        self.status_after_change(if checked {
            Change::ExemptAll
        } else {
            Change::ExemptNone
        })
    }

    pub fn checked_sids(&self) -> Vec<Sid> {
        self.rows
            .iter()
            .filter(|row| row.checked)
            .map(|row| row.app.sid.clone())
            .collect()
    }

    /// Call only after SetAppContainerConfig succeeds. Failure leaves edits retryable.
    pub fn mark_saved(&mut self) {
        for row in &mut self.rows {
            row.app.exempt_at_last_check = row.checked;
        }
        self.orphans.clear();
    }

    pub fn counter(&self) -> (usize, usize) {
        (
            self.rows.iter().filter(|row| row.checked).count(),
            self.rows.len(),
        )
    }

    pub fn counter_text(&self) -> String {
        let (checked, total) = self.counter();
        format!("{checked} / {total}")
    }

    pub fn sort_by(&mut self, column: Column, placeholders: &Placeholders) {
        self.sort.click(column);
        self.sort.apply(&mut self.rows, placeholders);
    }
}

/// Iterate current display order, never BTreeSet SID order.
pub fn copy_rows(rows: &[Row], selected: &BTreeSet<Sid>, placeholders: &Placeholders) -> String {
    let selected: Vec<_> = rows
        .iter()
        .filter(|row| selected.contains(&row.app.sid))
        .collect();
    let mut result = String::new();
    for row in &selected {
        result.push_str(if row.checked {
            "LoopbackExempt\t"
        } else {
            "LoopbackBlocked\t"
        });
        result.push_str(&row.columns(placeholders).join("\t"));
        if selected.len() > 1 {
            result.push_str("\r\n");
        }
    }
    result
}

pub fn copy_column(
    rows: &[Row],
    selected: &BTreeSet<Sid>,
    column: Column,
    placeholders: &Placeholders,
) -> String {
    let mut result = String::new();
    for row in rows.iter().filter(|row| selected.contains(&row.app.sid)) {
        result.push_str(&row.columns(placeholders)[column as usize]);
        result.push_str("\r\n"); // StringBuilder.AppendLine uses Environment.NewLine on Windows.
    }
    result
}

/// `-none` and `-deny *` bypass both enumeration and configuration reads.
pub fn clears_without_reading(command: &Command) -> bool {
    matches!(command, Command::None) || matches!(command, Command::Deny { name } if name == "*")
}

/// Only named filters need the current exemption baseline; wildcard allow replaces it.
pub fn needs_exemptions(command: &Command) -> bool {
    matches!(command, Command::Allow { name } | Command::Deny { name } if name != "*")
}

fn ordinal_ignore_case(text: &str) -> String {
    // OrdinalIgnoreCase uses one-to-one invariant uppercase, never expansions (ß != ss).
    text.chars()
        .map(|ch| {
            let mut uppercase = ch.to_uppercase();
            let first = uppercase.next().unwrap_or(ch);
            if uppercase.next().is_none() && ch != '\u{131}' {
                first
            } else {
                ch
            }
        })
        .collect()
}

/// Preserve enumeration order and omit orphans, like the original array-based CLI.
pub fn filter_exemptions(command: &Command, apps: &[AppContainer], exempted: &[Sid]) -> Vec<Sid> {
    if clears_without_reading(command) || matches!(command, Command::Syntax) {
        return Vec::new();
    }
    apps.iter()
        .filter(|app| match command {
            Command::All => true,
            Command::Allow { name } if name == "*" => true,
            Command::Allow { name } => {
                exempted.contains(&app.sid)
                    || app.name.as_ref().is_none_or(|value| {
                        value.is_empty()
                            || ordinal_ignore_case(value).contains(&ordinal_ignore_case(name))
                    })
            }
            Command::Deny { name } => {
                exempted.contains(&app.sid)
                    && app.name.as_ref().is_none_or(|value| {
                        value.is_empty()
                            || !ordinal_ignore_case(value).contains(&ordinal_ignore_case(name))
                    })
            }
            _ => false,
        })
        .map(|app| app.sid.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(id: u32, name: Option<&str>) -> AppContainer {
        AppContainer {
            sid: format!("S-1-15-2-{id}").parse().unwrap(),
            users: Vec::new(),
            name: name.map(str::to_owned),
            display_name: name.map(str::to_owned),
            description: None,
            package_full_name: None,
            binaries: Vec::new(),
            exempt_at_last_check: false,
        }
    }

    #[test]
    fn merge_keeps_first_metadata_and_position_and_sorts_all_users_ordinally() {
        let mut first = app(2, Some("first"));
        first.users = vec!["z".into()];
        let mut duplicate = app(2, Some("ignored"));
        duplicate.users = vec!["A".into(), "z".into()];
        let merged =
            crate::netiso::merge_records([first.clone(), app(1, Some("other")), duplicate]);
        assert_eq!(merged[0].name, first.name);
        assert_eq!(merged[0].users, ["A", "z", "z"]);
        assert_eq!(merged[1].sid, app(1, None).sid);
    }

    #[test]
    fn load_marks_baselines_sorts_and_collects_orphans_in_config_order() {
        let p = Placeholders::for_locale("en");
        let a = app(1, Some("z"));
        let b = app(2, Some("a"));
        let exemptions = vec![app(9, None).sid, b.sid.clone(), app(8, None).sid];
        let mut state = ListState::from_apps(vec![a, b.clone()], &exemptions, &p);
        assert_eq!(state.rows[0].app.sid, b.sid);
        assert!(state.rows[0].checked);
        assert!(!state.rows[1].checked);
        assert_eq!(
            state.orphans,
            [exemptions[0].clone(), exemptions[2].clone()]
        );
        assert!(!state.is_dirty());
        assert_eq!(state.counter_text(), "1 / 2");
        state.rows[0].checked = false;
        assert!(state.is_dirty()); // No mark_saved on a failed API call.
        assert!(state.rows[0].app.exempt_at_last_check);
        state.mark_saved();
        assert!(!state.is_dirty());
        assert!(state.orphans.is_empty());
    }

    #[test]
    fn null_empty_and_joined_values_render_differently() {
        let p = Placeholders::for_locale("en");
        let mut row = Row {
            app: app(1, None),
            checked: false,
        };
        assert_eq!(
            row.columns(&p),
            [
                "(No DisplayName)",
                "(No Description)",
                "(No Package Name)",
                "(No AC Name)",
                "S-1-15-2-1",
                "(No User SID)",
                "(None)"
            ]
        );
        row.app.display_name = Some(String::new());
        row.app.name = Some(String::new());
        row.app.users = vec![r"DOMAIN\user".into(), "b".into()];
        row.app.binaries = vec![String::new(), "c.exe".into()];
        let columns = row.columns(&p);
        assert_eq!(columns[0], "");
        assert_eq!(columns[3], "");
        assert_eq!(columns[5], r"DOMAIN\user; b");
        assert_eq!(columns[6], "; c.exe");
        assert!(row.highlights_user(r"DOMAIN\user"));
        assert!(!row.highlights_user(r"domain\user"));
    }

    #[test]
    fn status_bulk_dirty_clean_refresh_and_success_baselines() {
        let p = Placeholders::for_locale("en");
        let mut state =
            ListState::from_apps(vec![app(1, None), app(2, None)], &[app(9, None).sid], &p);
        assert_eq!(state.status_after_change(Change::Checked).key, None);
        assert_eq!(
            state.set_all(true),
            Status {
                key: Some("status.exempt_all"),
                orphan_removal_count: 1
            }
        );
        assert_eq!(state.counter(), (2, 2));
        assert_eq!(
            state.status_after_change(Change::Checked).key,
            Some("status.dirty")
        );
        state.refreshing = true;
        assert!(!state.is_dirty());
        state.refreshing = false;
        assert_eq!(state.set_all(false).key, None);
        state.set_all(true);
        state.mark_saved();
        assert_eq!(state.set_all(false).key, Some("status.exempt_none"));
        assert_eq!(state.set_all(true).key, None);
        assert_eq!(ListState::default().set_all(true).key, None);
    }

    #[test]
    fn sorting_compares_each_rendered_column_and_keeps_direction_on_column_change() {
        let p = Placeholders::for_locale("en");
        for column in [
            Column::DisplayName,
            Column::Description,
            Column::Package,
            Column::Name,
            Column::Sid,
            Column::Users,
            Column::Binaries,
        ] {
            let mut a = app(2, Some("z"));
            let mut b = app(1, Some("A"));
            a.description = Some("z".into());
            b.description = Some("A".into());
            a.package_full_name = Some("z".into());
            b.package_full_name = Some("A".into());
            a.users = vec!["z".into()];
            b.users = vec!["A".into()];
            a.binaries = vec!["z".into()];
            b.binaries = vec!["A".into()];
            let mut rows = vec![
                Row {
                    app: a,
                    checked: true,
                },
                Row {
                    app: b.clone(),
                    checked: false,
                },
            ];
            let mut sort = Sort {
                column,
                ascending: true,
            };
            sort.apply(&mut rows, &p);
            assert_eq!(rows[0].app.sid, b.sid);
            sort.click(column);
            sort.apply(&mut rows, &p);
            assert_eq!(rows[1].app.sid, b.sid);
        }
        let mut sort = Sort::default();
        sort.click(Column::DisplayName);
        assert!(!sort.ascending);
        sort.click(Column::Binaries);
        assert!(!sort.ascending);
        let state = ListState::from_apps(
            vec![app(1, Some("\u{e000}")), app(2, Some("\u{10000}"))],
            &[],
            &p,
        );
        assert_eq!(state.rows[0].app.sid, app(2, None).sid); // UTF-16 differs from UTF-8 here.
    }

    #[test]
    fn copy_uses_display_order_status_tokens_tabs_and_exact_crlf_rules() {
        let p = Placeholders::for_locale("en");
        let state = ListState::from_apps(
            vec![app(1, Some("z")), app(2, Some("a"))],
            &[app(2, None).sid],
            &p,
        );
        let mut selected = BTreeSet::new();
        assert_eq!(copy_rows(&state.rows, &selected, &p), "");
        selected.insert(app(2, None).sid);
        let expected = "LoopbackExempt\ta\t(No Description)\t(No Package Name)\ta\tS-1-15-2-2\t(No User SID)\t(None)";
        assert_eq!(copy_rows(&state.rows, &selected, &p), expected);
        assert_eq!(
            copy_column(&state.rows, &selected, Column::Name, &p),
            "a\r\n"
        );
        selected.insert(app(1, None).sid);
        assert_eq!(
            copy_rows(&state.rows, &selected, &p),
            format!(
                "{expected}\r\nLoopbackBlocked\tz\t(No Description)\t(No Package Name)\tz\tS-1-15-2-1\t(No User SID)\t(None)\r\n"
            )
        );
        assert_eq!(
            copy_column(&state.rows, &selected, Column::Name, &p),
            "a\r\nz\r\n"
        );
    }

    #[test]
    fn cli_filters_match_original_empty_names_wildcards_and_orphan_removal() {
        let apps = vec![
            app(1, Some("Store")),
            app(2, Some("other")),
            app(3, None),
            app(4, Some("")),
            app(5, Some("My STORE App")),
        ];
        let saved = vec![apps[0].sid.clone(), apps[1].sid.clone(), app(9, None).sid];
        let ids = |command| filter_exemptions(&command, &apps, &saved);
        assert_eq!(
            ids(Command::All),
            apps.iter().map(|a| a.sid.clone()).collect::<Vec<_>>()
        );
        assert_eq!(
            ids(Command::Allow {
                name: "sToRe".into()
            }),
            apps.iter().map(|a| a.sid.clone()).collect::<Vec<_>>()
        );
        assert_eq!(
            ids(Command::Deny {
                name: "STORE".into()
            }),
            [apps[1].sid.clone()]
        );
        assert!(ids(Command::None).is_empty());
        assert!(ids(Command::Deny { name: "*".into() }).is_empty());
        assert_eq!(ids(Command::Allow { name: "*".into() }).len(), 5);
        let all = apps.iter().map(|app| app.sid.clone()).collect::<Vec<_>>();
        assert_eq!(
            filter_exemptions(
                &Command::Deny {
                    name: "store".into()
                },
                &apps,
                &all
            ),
            all[1..4]
        );
        assert_eq!(ordinal_ignore_case("Straße"), "STRAßE");
        assert_eq!(ordinal_ignore_case("ёж"), "ЁЖ");
    }
}
