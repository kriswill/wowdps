//! The terminal keymap: crossterm key events translated into core `Action`s.
//! Lives here rather than in core so the state machine stays input-agnostic.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use wowdps_model::Action;
use wowdps_model::View;

pub fn action_for(key: KeyEvent) -> Option<Action> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    Some(match key.code {
        KeyCode::Char('c') if ctrl => Action::Quit,
        KeyCode::Char('q') => Action::Quit,
        KeyCode::Char('d') => Action::SetView(View::Damage),
        KeyCode::Char('h') => Action::SetView(View::Healing),
        KeyCode::Char('i') => Action::SetView(View::Interrupts),
        KeyCode::Char('c') => Action::SetView(View::CrowdControl),
        KeyCode::Char('x') => Action::SetView(View::Dispels),
        // Shift-K, because lowercase k is vim-style "move up".
        KeyCode::Char('K') => Action::SetView(View::Deaths),
        KeyCode::Char('j') | KeyCode::Down => Action::Down,
        KeyCode::Char('k') | KeyCode::Up => Action::Up,
        KeyCode::Char('[') | KeyCode::Left => Action::OlderSegment,
        KeyCode::Char(']') | KeyCode::Right => Action::NewerSegment,
        KeyCode::Enter => Action::Open,
        KeyCode::Esc => Action::Back,
        KeyCode::Tab | KeyCode::BackTab => Action::SwapPane,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ch(c: char) -> Option<Action> {
        action_for(key(KeyCode::Char(c)))
    }

    #[test]
    fn every_view_has_a_key() {
        assert_eq!(ch('d'), Some(Action::SetView(View::Damage)));
        assert_eq!(ch('h'), Some(Action::SetView(View::Healing)));
        assert_eq!(ch('i'), Some(Action::SetView(View::Interrupts)));
        assert_eq!(ch('c'), Some(Action::SetView(View::CrowdControl)));
        assert_eq!(ch('x'), Some(Action::SetView(View::Dispels)));
        assert_eq!(ch('K'), Some(Action::SetView(View::Deaths)));
    }

    #[test]
    fn movement_and_control_keys() {
        assert_eq!(ch('j'), Some(Action::Down));
        assert_eq!(ch('k'), Some(Action::Up));
        assert_eq!(action_for(key(KeyCode::Down)), Some(Action::Down));
        assert_eq!(action_for(key(KeyCode::Up)), Some(Action::Up));
        assert_eq!(ch('['), Some(Action::OlderSegment));
        assert_eq!(ch(']'), Some(Action::NewerSegment));
        assert_eq!(action_for(key(KeyCode::Enter)), Some(Action::Open));
        assert_eq!(action_for(key(KeyCode::Esc)), Some(Action::Back));
        assert_eq!(action_for(key(KeyCode::Tab)), Some(Action::SwapPane));
        assert_eq!(ch('q'), Some(Action::Quit));
    }

    #[test]
    fn ctrl_c_quits_since_raw_mode_swallows_the_signal() {
        let e = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(action_for(e), Some(Action::Quit));
    }

    #[test]
    fn unknown_keys_are_ignored() {
        assert_eq!(ch('z'), None);
        assert_eq!(action_for(key(KeyCode::F(5))), None);
    }
}
