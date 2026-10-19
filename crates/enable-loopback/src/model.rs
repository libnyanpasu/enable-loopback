//! Pure list state, dirty detection, and future sorting/selection/copy/filter rules (§3.3/§6.2).

use crate::netiso::AppContainer;

#[derive(Clone, Debug)]
pub struct Row {
    pub app: AppContainer,
    pub checked: bool,
}

#[derive(Debug, Default)]
pub struct ListState {
    pub rows: Vec<Row>,
    /// Suppresses dirty notifications while refresh replaces rows (§3.3).
    pub refreshing: bool,
}

impl ListState {
    pub fn is_dirty(&self) -> bool {
        !self.refreshing
            && self
                .rows
                .iter()
                .any(|row| row.checked != row.app.exempt_at_last_check)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::netiso::Sid;

    fn row(saved: bool, checked: bool) -> Row {
        Row {
            app: AppContainer {
                sid: Sid(vec![]),
                users: vec![],
                name: None,
                display_name: None,
                description: None,
                package_full_name: None,
                binaries: vec![],
                exempt_at_last_check: saved,
            },
            checked,
        }
    }

    #[test]
    fn empty_and_matching_rows_are_clean() {
        assert!(!ListState::default().is_dirty());
        assert!(
            !ListState {
                rows: vec![row(true, true), row(false, false)],
                refreshing: false
            }
            .is_dirty()
        );
    }

    #[test]
    fn either_direction_is_dirty_and_reverting_cleans() {
        for saved in [true, false] {
            let mut state = ListState {
                rows: vec![row(!saved, !saved), row(saved, !saved)],
                refreshing: false,
            };
            assert!(state.is_dirty());
            // A failed save must leave the baseline and dirty state intact.
            assert_eq!(state.rows[1].app.exempt_at_last_check, saved);
            state.rows[1].checked = saved;
            assert!(!state.is_dirty());
        }
    }

    #[test]
    fn refresh_suppresses_dirty_check_until_population_finishes() {
        let mut state = ListState {
            rows: vec![row(false, true)],
            refreshing: true,
        };
        assert!(!state.is_dirty());
        state.refreshing = false;
        assert!(state.is_dirty());
    }
}
