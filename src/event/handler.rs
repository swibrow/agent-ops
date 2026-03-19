use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::{ActiveView, App};
use crate::event::action::Action;

pub fn handle_key_event(key: KeyEvent, app: &App) -> Action {
    // If search is active, handle text input
    if app.search_active {
        return handle_search_key(key);
    }

    // If quit confirmation is showing
    if app.show_quit_confirm {
        return match key.code {
            KeyCode::Char('y') | KeyCode::Char('q') | KeyCode::Enter => Action::Quit,
            _ => Action::CancelQuit,
        };
    }

    // Global keys first
    match key.code {
        KeyCode::Char('q') => return Action::RequestQuit,
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return Action::Quit,
        KeyCode::Char('1') => return Action::SwitchTab(ActiveView::Dashboard),
        KeyCode::Char('2') => return Action::SwitchTab(ActiveView::Projects),
        KeyCode::Char('3') => return Action::SwitchTab(ActiveView::History),
        KeyCode::Tab | KeyCode::Char('l') => return Action::NextTab,
        KeyCode::BackTab | KeyCode::Char('h') => return Action::PrevTab,
        KeyCode::Char('?') => return Action::ToggleHelp,
        KeyCode::Char('/') => return Action::EnterSearch,
        _ => {}
    }

    // If help overlay is showing, any other key closes it
    if app.show_help {
        return Action::CloseOverlay;
    }

    // If detail overlay is showing
    if app.show_detail {
        return match key.code {
            KeyCode::Esc => Action::CloseOverlay,
            KeyCode::Char('r') => Action::ResumeSession,
            KeyCode::Char('y') => Action::CopySessionId,
            _ => Action::None,
        };
    }

    // View-specific keys
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => Action::SelectNext,
        KeyCode::Char('k') | KeyCode::Up => Action::SelectPrev,
        KeyCode::Char('g') => Action::SelectFirst,
        KeyCode::Char('G') => Action::SelectLast,
        KeyCode::Enter => Action::Confirm,
        KeyCode::Char(' ') => Action::TogglePreview,
        KeyCode::Char('r') => Action::ResumeSession,
        KeyCode::Char('R') => Action::Refresh,
        KeyCode::Char('s') => match app.active_view {
            ActiveView::Projects => Action::CycleSort,
            _ => Action::None,
        },
        KeyCode::Char('f') => match app.active_view {
            ActiveView::Projects | ActiveView::History => Action::FilterStale,
            _ => Action::None,
        },
        KeyCode::Char('F') => Action::ClearFilter,
        KeyCode::Char('e') => match app.active_view {
            ActiveView::Projects => Action::OpenInEditor,
            _ => Action::None,
        },
        KeyCode::Esc => Action::CloseOverlay,
        _ => Action::None,
    }
}

fn handle_search_key(key: KeyEvent) -> Action {
    match key.code {
        KeyCode::Esc => Action::ExitSearch,
        KeyCode::Enter => Action::ExitSearch,
        KeyCode::Backspace => Action::SearchBackspace,
        KeyCode::Char(c) => Action::SearchInput(c),
        _ => Action::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn key_with_mod(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    fn default_app() -> App {
        App::new()
    }

    // ── Global keys ──────────────────────────────────────────────

    #[test]
    fn q_produces_request_quit() {
        let app = default_app();
        assert_eq!(
            handle_key_event(key(KeyCode::Char('q')), &app),
            Action::RequestQuit
        );
    }

    #[test]
    fn ctrl_c_produces_quit() {
        let app = default_app();
        assert_eq!(
            handle_key_event(
                key_with_mod(KeyCode::Char('c'), KeyModifiers::CONTROL),
                &app
            ),
            Action::Quit,
        );
    }

    #[test]
    fn slash_produces_enter_search() {
        let app = default_app();
        assert_eq!(
            handle_key_event(key(KeyCode::Char('/')), &app),
            Action::EnterSearch,
        );
    }

    #[test]
    fn question_mark_toggles_help() {
        let app = default_app();
        assert_eq!(
            handle_key_event(key(KeyCode::Char('?')), &app),
            Action::ToggleHelp,
        );
    }

    #[test]
    fn number_keys_switch_tabs() {
        let app = default_app();
        assert_eq!(
            handle_key_event(key(KeyCode::Char('1')), &app),
            Action::SwitchTab(ActiveView::Dashboard),
        );
        assert_eq!(
            handle_key_event(key(KeyCode::Char('2')), &app),
            Action::SwitchTab(ActiveView::Projects),
        );
        assert_eq!(
            handle_key_event(key(KeyCode::Char('3')), &app),
            Action::SwitchTab(ActiveView::History),
        );
    }

    #[test]
    fn tab_produces_next_tab() {
        let app = default_app();
        assert_eq!(handle_key_event(key(KeyCode::Tab), &app), Action::NextTab);
    }

    #[test]
    fn backtab_produces_prev_tab() {
        let app = default_app();
        assert_eq!(
            handle_key_event(key(KeyCode::BackTab), &app),
            Action::PrevTab
        );
    }

    #[test]
    fn h_produces_prev_tab() {
        let app = default_app();
        assert_eq!(
            handle_key_event(key(KeyCode::Char('h')), &app),
            Action::PrevTab
        );
    }

    #[test]
    fn l_produces_next_tab() {
        let app = default_app();
        assert_eq!(
            handle_key_event(key(KeyCode::Char('l')), &app),
            Action::NextTab
        );
    }

    // ── Navigation keys ──────────────────────────────────────────

    #[test]
    fn j_and_down_produce_select_next() {
        let app = default_app();
        assert_eq!(
            handle_key_event(key(KeyCode::Char('j')), &app),
            Action::SelectNext
        );
        assert_eq!(
            handle_key_event(key(KeyCode::Down), &app),
            Action::SelectNext
        );
    }

    #[test]
    fn k_and_up_produce_select_prev() {
        let app = default_app();
        assert_eq!(
            handle_key_event(key(KeyCode::Char('k')), &app),
            Action::SelectPrev
        );
        assert_eq!(handle_key_event(key(KeyCode::Up), &app), Action::SelectPrev);
    }

    #[test]
    fn g_produces_select_first() {
        let app = default_app();
        assert_eq!(
            handle_key_event(key(KeyCode::Char('g')), &app),
            Action::SelectFirst
        );
    }

    #[test]
    fn big_g_produces_select_last() {
        let app = default_app();
        assert_eq!(
            handle_key_event(key(KeyCode::Char('G')), &app),
            Action::SelectLast
        );
    }

    #[test]
    fn enter_produces_confirm() {
        let app = default_app();
        assert_eq!(handle_key_event(key(KeyCode::Enter), &app), Action::Confirm);
    }

    #[test]
    fn space_produces_toggle_preview() {
        let app = default_app();
        assert_eq!(
            handle_key_event(key(KeyCode::Char(' ')), &app),
            Action::TogglePreview
        );
    }

    #[test]
    fn big_r_produces_refresh() {
        let app = default_app();
        assert_eq!(
            handle_key_event(key(KeyCode::Char('R')), &app),
            Action::Refresh
        );
    }

    // ── Quit confirmation ─────────────────────────────────────────

    #[test]
    fn y_confirms_quit() {
        let mut app = default_app();
        app.show_quit_confirm = true;
        assert_eq!(
            handle_key_event(key(KeyCode::Char('y')), &app),
            Action::Quit
        );
    }

    #[test]
    fn q_confirms_quit() {
        let mut app = default_app();
        app.show_quit_confirm = true;
        assert_eq!(
            handle_key_event(key(KeyCode::Char('q')), &app),
            Action::Quit
        );
    }

    #[test]
    fn enter_confirms_quit() {
        let mut app = default_app();
        app.show_quit_confirm = true;
        assert_eq!(handle_key_event(key(KeyCode::Enter), &app), Action::Quit);
    }

    #[test]
    fn other_key_cancels_quit() {
        let mut app = default_app();
        app.show_quit_confirm = true;
        assert_eq!(
            handle_key_event(key(KeyCode::Char('n')), &app),
            Action::CancelQuit
        );
        assert_eq!(
            handle_key_event(key(KeyCode::Esc), &app),
            Action::CancelQuit
        );
    }

    // ── Search mode keys ─────────────────────────────────────────

    #[test]
    fn search_active_chars_produce_search_input() {
        let mut app = default_app();
        app.search_active = true;
        assert_eq!(
            handle_key_event(key(KeyCode::Char('a')), &app),
            Action::SearchInput('a'),
        );
        assert_eq!(
            handle_key_event(key(KeyCode::Char('z')), &app),
            Action::SearchInput('z'),
        );
    }

    #[test]
    fn search_active_backspace_produces_search_backspace() {
        let mut app = default_app();
        app.search_active = true;
        assert_eq!(
            handle_key_event(key(KeyCode::Backspace), &app),
            Action::SearchBackspace,
        );
    }

    #[test]
    fn search_active_esc_exits_search() {
        let mut app = default_app();
        app.search_active = true;
        assert_eq!(
            handle_key_event(key(KeyCode::Esc), &app),
            Action::ExitSearch,
        );
    }

    #[test]
    fn search_active_enter_exits_search() {
        let mut app = default_app();
        app.search_active = true;
        assert_eq!(
            handle_key_event(key(KeyCode::Enter), &app),
            Action::ExitSearch,
        );
    }

    #[test]
    fn search_active_q_produces_search_input_not_quit() {
        let mut app = default_app();
        app.search_active = true;
        assert_eq!(
            handle_key_event(key(KeyCode::Char('q')), &app),
            Action::SearchInput('q'),
        );
    }

    // ── View-specific keys ───────────────────────────────────────

    #[test]
    fn e_on_projects_produces_open_in_editor() {
        let mut app = default_app();
        app.active_view = ActiveView::Projects;
        assert_eq!(
            handle_key_event(key(KeyCode::Char('e')), &app),
            Action::OpenInEditor,
        );
    }

    #[test]
    fn e_on_dashboard_produces_none() {
        let app = default_app(); // Dashboard is the default view
        assert_eq!(
            handle_key_event(key(KeyCode::Char('e')), &app),
            Action::None,
        );
    }

    #[test]
    fn s_on_projects_produces_cycle_sort() {
        let mut app = default_app();
        app.active_view = ActiveView::Projects;
        assert_eq!(
            handle_key_event(key(KeyCode::Char('s')), &app),
            Action::CycleSort,
        );
    }

    #[test]
    fn s_on_dashboard_produces_none() {
        let app = default_app();
        assert_eq!(
            handle_key_event(key(KeyCode::Char('s')), &app),
            Action::None,
        );
    }

    #[test]
    fn f_on_projects_produces_filter_stale() {
        let mut app = default_app();
        app.active_view = ActiveView::Projects;
        assert_eq!(
            handle_key_event(key(KeyCode::Char('f')), &app),
            Action::FilterStale,
        );
    }

    #[test]
    fn f_on_history_produces_filter_stale() {
        let mut app = default_app();
        app.active_view = ActiveView::History;
        assert_eq!(
            handle_key_event(key(KeyCode::Char('f')), &app),
            Action::FilterStale,
        );
    }

    #[test]
    fn f_on_dashboard_produces_none() {
        let app = default_app();
        assert_eq!(
            handle_key_event(key(KeyCode::Char('f')), &app),
            Action::None,
        );
    }

    #[test]
    fn big_f_produces_clear_filter() {
        let app = default_app();
        assert_eq!(
            handle_key_event(key(KeyCode::Char('F')), &app),
            Action::ClearFilter,
        );
    }

    // ── Help overlay ─────────────────────────────────────────────

    #[test]
    fn any_key_closes_help_overlay() {
        let mut app = default_app();
        app.show_help = true;
        // A key that is not a global key should close the overlay
        assert_eq!(
            handle_key_event(key(KeyCode::Char('x')), &app),
            Action::CloseOverlay,
        );
    }

    #[test]
    fn global_keys_still_work_with_help_showing() {
        let mut app = default_app();
        app.show_help = true;
        // 'q' is a global key and should still request quit
        assert_eq!(
            handle_key_event(key(KeyCode::Char('q')), &app),
            Action::RequestQuit,
        );
    }

    // ── Detail overlay ───────────────────────────────────────────

    #[test]
    fn esc_closes_detail_overlay() {
        let mut app = default_app();
        app.show_detail = true;
        assert_eq!(
            handle_key_event(key(KeyCode::Esc), &app),
            Action::CloseOverlay,
        );
    }

    #[test]
    fn r_on_detail_overlay_resumes_session() {
        let mut app = default_app();
        app.show_detail = true;
        assert_eq!(
            handle_key_event(key(KeyCode::Char('r')), &app),
            Action::ResumeSession,
        );
    }

    #[test]
    fn y_on_detail_overlay_copies_session_id() {
        let mut app = default_app();
        app.show_detail = true;
        assert_eq!(
            handle_key_event(key(KeyCode::Char('y')), &app),
            Action::CopySessionId,
        );
    }

    #[test]
    fn unknown_key_on_detail_overlay_produces_none() {
        let mut app = default_app();
        app.show_detail = true;
        assert_eq!(
            handle_key_event(key(KeyCode::Char('x')), &app),
            Action::None,
        );
    }
}
