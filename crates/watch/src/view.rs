//! View state of `watch`: which pane has focus, what is selected, scrolling,
//! and what each key does. No drawing happens here.

use crate::state::{RowId, WatchState};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::widgets::ListState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    Tree,
    Feed,
    Details,
}

impl Pane {
    pub fn next(self) -> Pane {
        match self {
            Pane::Tree => Pane::Feed,
            Pane::Feed => Pane::Details,
            Pane::Details => Pane::Tree,
        }
    }
    pub fn prev(self) -> Pane {
        match self {
            Pane::Tree => Pane::Details,
            Pane::Feed => Pane::Tree,
            Pane::Details => Pane::Feed,
        }
    }
}

/// The full screen details view opened with enter.
#[derive(Debug, Clone, PartialEq)]
pub struct Overlay {
    pub id: RowId,
    pub scroll: u16,
    /// Unified diff from the parent, one line per entry. Filled in by the app.
    pub diff: Vec<String>,
}

/// What the key handler wants the app to do.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    None,
    Quit,
    /// The overlay was opened. The app should load the diff for this row.
    LoadDiff(RowId),
}

/// State of a child process started with `watch --run`.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum ChildStatus {
    #[default]
    None,
    Running(String),
    Exited {
        ok: bool,
        text: String,
    },
}

impl ChildStatus {
    pub fn is_running(&self) -> bool {
        matches!(self, ChildStatus::Running(_))
    }
}

#[derive(Debug)]
pub struct ViewState {
    pub focus: Pane,
    pub selected: Option<RowId>,
    /// Keep the selection on the newest row as events arrive.
    pub follow: bool,
    pub overlay: Option<Overlay>,
    pub details_scroll: u16,
    pub tree_list: ListState,
    pub feed_list: ListState,
    /// Counts redraws, for the small activity spinner.
    pub tick: u64,
    /// `q` was pressed once while a child process is running.
    pub quit_armed: bool,
}

impl Default for ViewState {
    fn default() -> Self {
        ViewState {
            focus: Pane::Tree,
            selected: None,
            follow: true,
            overlay: None,
            details_scroll: 0,
            tree_list: ListState::default(),
            feed_list: ListState::default(),
            tick: 0,
            quit_armed: false,
        }
    }
}

impl ViewState {
    /// Call after every state update: keeps the selection valid and moves it
    /// to the newest row while following.
    pub fn sync(&mut self, state: &WatchState) {
        let valid = self
            .selected
            .as_ref()
            .is_some_and(|id| state.row(id).is_some());
        if self.follow || !valid {
            if let Some(row) = state.newest_row() {
                if self.selected.as_ref() != Some(&row.id) {
                    self.details_scroll = 0;
                }
                self.selected = Some(row.id.clone());
            } else {
                self.selected = None;
            }
        }
    }

    fn select(&mut self, id: RowId) {
        if self.selected.as_ref() != Some(&id) {
            self.details_scroll = 0;
        }
        self.selected = Some(id);
    }

    fn move_tree(&mut self, state: &WatchState, delta: isize) {
        if state.rows.is_empty() {
            return;
        }
        let cur = self
            .selected
            .as_ref()
            .and_then(|id| state.row_index(id))
            .unwrap_or(0) as isize;
        let next = (cur + delta).clamp(0, state.rows.len() as isize - 1) as usize;
        self.follow = false;
        self.select(state.rows[next].id.clone());
    }

    fn move_feed(&mut self, state: &WatchState, delta: isize) {
        if state.feed.is_empty() {
            return;
        }
        let cur = self
            .selected
            .as_ref()
            .and_then(|id| state.feed.iter().position(|f| &f.target == id));
        let next = match cur {
            Some(c) => (c as isize + delta).clamp(0, state.feed.len() as isize - 1) as usize,
            None => 0,
        };
        self.follow = false;
        self.select(state.feed[next].target.clone());
    }

    fn scroll(&mut self, delta: i32) {
        let target = match self.overlay.as_mut() {
            Some(o) => &mut o.scroll,
            None => &mut self.details_scroll,
        };
        *target = (i32::from(*target) + delta).clamp(0, i32::from(u16::MAX)) as u16;
    }

    /// Handle one key press. `child_running` is true while `watch --run`
    /// has a child process going.
    pub fn handle_key(&mut self, state: &WatchState, key: KeyEvent, child_running: bool) -> Action {
        let ctrl_c =
            key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c');
        let quit_key = ctrl_c || (self.overlay.is_none() && key.code == KeyCode::Char('q'));
        if quit_key {
            if child_running && !self.quit_armed {
                self.quit_armed = true;
                return Action::None;
            }
            return Action::Quit;
        }
        self.quit_armed = false;

        if self.overlay.is_some() {
            match key.code {
                KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => self.overlay = None,
                KeyCode::Down | KeyCode::Char('j') => self.scroll(1),
                KeyCode::Up | KeyCode::Char('k') => self.scroll(-1),
                KeyCode::PageDown | KeyCode::Char(' ') => self.scroll(10),
                KeyCode::PageUp => self.scroll(-10),
                KeyCode::Home | KeyCode::Char('g') => self.scroll(-i32::from(u16::MAX)),
                _ => {}
            }
            return Action::None;
        }

        match key.code {
            KeyCode::Tab => self.focus = self.focus.next(),
            KeyCode::BackTab => self.focus = self.focus.prev(),
            KeyCode::Char('f') => {
                self.follow = !self.follow;
                if self.follow {
                    self.sync(state);
                }
            }
            KeyCode::Enter => {
                if let Some(id) = self.selected.clone() {
                    if state.row(&id).is_some() {
                        self.overlay = Some(Overlay {
                            id: id.clone(),
                            scroll: 0,
                            diff: Vec::new(),
                        });
                        return Action::LoadDiff(id);
                    }
                }
            }
            KeyCode::Down | KeyCode::Char('j') => self.step(state, 1),
            KeyCode::Up | KeyCode::Char('k') => self.step(state, -1),
            KeyCode::PageDown => self.step(state, 10),
            KeyCode::PageUp => self.step(state, -10),
            KeyCode::Home | KeyCode::Char('g') => self.step(state, -1_000_000),
            KeyCode::End | KeyCode::Char('G') => self.step(state, 1_000_000),
            _ => {}
        }
        Action::None
    }

    fn step(&mut self, state: &WatchState, delta: isize) {
        match self.focus {
            Pane::Tree => self.move_tree(state, delta),
            Pane::Feed => self.move_feed(state, delta),
            Pane::Details => self.scroll(delta.clamp(-1000, 1000) as i32),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::RowStatus;
    use crate::testkit::*;
    use ratatui::crossterm::event::KeyEventKind;

    fn key(code: KeyCode) -> KeyEvent {
        let mut k = KeyEvent::new(code, KeyModifiers::NONE);
        k.kind = KeyEventKind::Press;
        k
    }

    fn setup() -> (WatchState, ViewState) {
        let s = build(&story());
        let mut v = ViewState::default();
        v.sync(&s);
        (s, v)
    }

    #[test]
    fn follow_starts_on_the_newest_row() {
        let (s, v) = setup();
        assert!(v.follow);
        assert_eq!(v.selected, Some(s.newest_row().unwrap().id.clone()));
    }

    #[test]
    fn moving_in_the_tree_stops_following_and_clamps() {
        let (s, mut v) = setup();
        v.handle_key(&s, key(KeyCode::Char('g')), false);
        assert!(!v.follow);
        assert_eq!(v.selected, Some(s.rows[0].id.clone()));
        v.handle_key(&s, key(KeyCode::Up), false);
        assert_eq!(v.selected, Some(s.rows[0].id.clone()));
        v.handle_key(&s, key(KeyCode::Char('j')), false);
        assert_eq!(v.selected, Some(s.rows[1].id.clone()));
        v.handle_key(&s, key(KeyCode::Char('G')), false);
        assert_eq!(v.selected, Some(s.rows.last().unwrap().id.clone()));
        v.handle_key(&s, key(KeyCode::Down), false);
        assert_eq!(v.selected, Some(s.rows.last().unwrap().id.clone()));
        v.handle_key(&s, key(KeyCode::Char('k')), false);
        assert_eq!(v.selected, Some(s.rows[s.rows.len() - 2].id.clone()));
    }

    #[test]
    fn f_toggles_follow_and_jumps_to_the_newest() {
        let (s, mut v) = setup();
        v.handle_key(&s, key(KeyCode::Char('g')), false);
        v.handle_key(&s, key(KeyCode::Char('f')), false);
        assert!(v.follow);
        assert_eq!(v.selected, Some(s.newest_row().unwrap().id.clone()));
        v.handle_key(&s, key(KeyCode::Char('f')), false);
        assert!(!v.follow);
    }

    #[test]
    fn selection_sticks_when_not_following_and_moves_when_following() {
        let (s, mut v) = setup();
        v.handle_key(&s, key(KeyCode::Char('g')), false);
        let mut more = story();
        more.push(proposed('e', 'f', "Another idea."));
        let s2 = build(&more);
        v.sync(&s2);
        assert_eq!(v.selected, Some(s2.rows[0].id.clone()));
        v.handle_key(&s2, key(KeyCode::Char('f')), false);
        assert_eq!(v.selected, Some(s2.newest_row().unwrap().id.clone()));
        let mut even_more = more.clone();
        even_more.push(proposed('f', 'g', "Yet another."));
        let s3 = build(&even_more);
        v.sync(&s3);
        assert_eq!(v.selected, Some(s3.newest_row().unwrap().id.clone()));
    }

    #[test]
    fn tab_cycles_panes_both_ways() {
        let (s, mut v) = setup();
        assert_eq!(v.focus, Pane::Tree);
        v.handle_key(&s, key(KeyCode::Tab), false);
        assert_eq!(v.focus, Pane::Feed);
        v.handle_key(&s, key(KeyCode::Tab), false);
        assert_eq!(v.focus, Pane::Details);
        v.handle_key(&s, key(KeyCode::Tab), false);
        assert_eq!(v.focus, Pane::Tree);
        v.handle_key(&s, key(KeyCode::BackTab), false);
        assert_eq!(v.focus, Pane::Details);
    }

    #[test]
    fn feed_focus_moves_through_the_feed_and_selects_its_target() {
        let (s, mut v) = setup();
        v.handle_key(&s, key(KeyCode::Tab), false);
        v.handle_key(&s, key(KeyCode::Char('g')), false);
        assert_eq!(v.selected, Some(s.feed[0].target.clone()));
        v.handle_key(&s, key(KeyCode::Char('j')), false);
        assert_eq!(v.selected, Some(s.feed[1].target.clone()));
        v.handle_key(&s, key(KeyCode::Char('G')), false);
        assert_eq!(v.selected, Some(s.feed.last().unwrap().target.clone()));
    }

    #[test]
    fn details_pane_scrolls_instead_of_moving_the_selection() {
        let (s, mut v) = setup();
        v.handle_key(&s, key(KeyCode::Tab), false);
        v.handle_key(&s, key(KeyCode::Tab), false);
        let before = v.selected.clone();
        v.handle_key(&s, key(KeyCode::Char('j')), false);
        v.handle_key(&s, key(KeyCode::Char('j')), false);
        assert_eq!(v.details_scroll, 2);
        assert_eq!(v.selected, before);
        v.handle_key(&s, key(KeyCode::Char('k')), false);
        assert_eq!(v.details_scroll, 1);
        v.handle_key(&s, key(KeyCode::Char('g')), false);
        assert_eq!(v.details_scroll, 0);
    }

    #[test]
    fn enter_opens_the_overlay_and_asks_for_the_diff() {
        let (s, mut v) = setup();
        let want = v.selected.clone().unwrap();
        let action = v.handle_key(&s, key(KeyCode::Enter), false);
        assert_eq!(action, Action::LoadDiff(want.clone()));
        assert_eq!(v.overlay.as_ref().unwrap().id, want);
        // Keys scroll the overlay, and q closes it instead of quitting.
        v.handle_key(&s, key(KeyCode::Char('j')), false);
        assert_eq!(v.overlay.as_ref().unwrap().scroll, 1);
        assert_eq!(
            v.handle_key(&s, key(KeyCode::Char('q')), false),
            Action::None
        );
        assert!(v.overlay.is_none());
        v.handle_key(&s, key(KeyCode::Enter), false);
        v.handle_key(&s, key(KeyCode::Esc), false);
        assert!(v.overlay.is_none());
    }

    #[test]
    fn enter_with_nothing_selected_does_nothing() {
        let s = WatchState::build(
            &[],
            crate::state::ChainStatus::ok(0),
            crate::state::ConfigView::default(),
        );
        let mut v = ViewState::default();
        v.sync(&s);
        assert_eq!(v.handle_key(&s, key(KeyCode::Enter), false), Action::None);
        assert!(v.overlay.is_none());
        v.handle_key(&s, key(KeyCode::Char('j')), false);
        assert!(v.selected.is_none());
    }

    #[test]
    fn q_quits_and_ctrl_c_quits() {
        let (s, mut v) = setup();
        assert_eq!(
            v.handle_key(&s, key(KeyCode::Char('q')), false),
            Action::Quit
        );
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(v.handle_key(&s, ctrl_c, false), Action::Quit);
    }

    #[test]
    fn q_needs_a_second_press_while_a_child_runs() {
        let (s, mut v) = setup();
        assert_eq!(
            v.handle_key(&s, key(KeyCode::Char('q')), true),
            Action::None
        );
        assert!(v.quit_armed);
        assert_eq!(
            v.handle_key(&s, key(KeyCode::Char('q')), true),
            Action::Quit
        );
        // Any other key disarms.
        let (s, mut v) = setup();
        v.handle_key(&s, key(KeyCode::Char('q')), true);
        v.handle_key(&s, key(KeyCode::Char('j')), true);
        assert!(!v.quit_armed);
        assert_eq!(
            v.handle_key(&s, key(KeyCode::Char('q')), true),
            Action::None
        );
    }

    #[test]
    fn selection_survives_in_a_row_that_exists_and_resets_when_it_does_not() {
        let (s, mut v) = setup();
        v.follow = false;
        v.selected = Some(RowId::Refused(9999));
        v.sync(&s);
        assert!(v.selected.is_some() && s.row(v.selected.as_ref().unwrap()).is_some());
        assert_eq!(
            s.rows
                .iter()
                .filter(|r| r.status == RowStatus::Root)
                .count(),
            1
        );
    }
}
