use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use std::time::Duration;

pub enum Action {
    Quit,
    Up,
    Down,
    Confirm,
    ModeSwitch,
    Tab,
    Char(char),
    Backspace,
    OpenLog,
    PageUp,
    PageDown,
    None,
}

pub fn read() -> Result<Option<Action>, Box<dyn std::error::Error>> {
    if !event::poll(Duration::from_millis(100))? {
        return Ok(None);
    }

    let Event::Key(key) = event::read()? else {
        return Ok(None);
    };

    if key.kind != KeyEventKind::Press {
        return Ok(None);
    }

    match key.code {
        KeyCode::Char('c') if key.modifiers == event::KeyModifiers::CONTROL => {
            Ok(Some(Action::Quit))
        }
        KeyCode::Char('d') if key.modifiers == event::KeyModifiers::CONTROL => {
            Ok(Some(Action::Quit))
        }
        KeyCode::Esc => Ok(Some(Action::ModeSwitch)),
        KeyCode::Char(':') => Ok(Some(Action::ModeSwitch)),
        KeyCode::Up if key.modifiers == event::KeyModifiers::SHIFT => Ok(Some(Action::PageUp)),
        KeyCode::Down if key.modifiers == event::KeyModifiers::SHIFT => Ok(Some(Action::PageDown)),
        KeyCode::Up => Ok(Some(Action::Up)),
        KeyCode::Down => Ok(Some(Action::Down)),
        KeyCode::Enter => Ok(Some(Action::Confirm)),
        KeyCode::Tab => Ok(Some(Action::Tab)),
        KeyCode::Backspace => Ok(Some(Action::Backspace)),
        KeyCode::F(12) => Ok(Some(Action::OpenLog)),
        KeyCode::PageUp => Ok(Some(Action::PageUp)),
        KeyCode::PageDown => Ok(Some(Action::PageDown)),
        KeyCode::Char(c) => Ok(Some(Action::Char(c))),
        _ => Ok(Some(Action::None)),
    }
}
