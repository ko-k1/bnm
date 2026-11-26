//! Small single-line text input used by prompts, filters, and the edit form.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Debug, Clone, Default)]
pub struct TextInput {
    chars: Vec<char>,
    cursor: usize,
    masked: bool,
}

impl TextInput {
    pub fn new(value: &str) -> Self {
        let chars: Vec<char> = value.chars().collect();
        Self {
            cursor: chars.len(),
            chars,
            masked: false,
        }
    }

    pub fn secret() -> Self {
        Self {
            masked: true,
            ..Self::default()
        }
    }

    pub fn value(&self) -> String {
        self.chars.iter().collect()
    }

    pub fn trimmed(&self) -> String {
        self.value().trim().to_string()
    }

    pub fn is_empty(&self) -> bool {
        self.chars.is_empty()
    }

    pub fn clear(&mut self) {
        self.chars.clear();
        self.cursor = 0;
    }

    pub fn insert(&mut self, character: char) {
        self.chars.insert(self.cursor, character);
        self.cursor += 1;
    }

    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            self.chars.remove(self.cursor);
        }
    }

    pub fn delete(&mut self) {
        if self.cursor < self.chars.len() {
            self.chars.remove(self.cursor);
        }
    }

    pub fn left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn right(&mut self) {
        if self.cursor < self.chars.len() {
            self.cursor += 1;
        }
    }

    pub fn home(&mut self) {
        self.cursor = 0;
    }

    pub fn end(&mut self) {
        self.cursor = self.chars.len();
    }

    pub fn delete_word(&mut self) {
        while self.cursor > 0 && self.chars[self.cursor - 1].is_whitespace() {
            self.backspace();
        }
        while self.cursor > 0 && !self.chars[self.cursor - 1].is_whitespace() {
            self.backspace();
        }
    }

    pub fn current(&self) -> Option<char> {
        self.chars.get(self.cursor).copied()
    }

    pub fn before_display(&self) -> String {
        self.chars[..self.cursor]
            .iter()
            .map(|character| self.display_char(*character))
            .collect()
    }

    pub fn after_display(&self) -> String {
        self.chars[self.cursor..]
            .iter()
            .map(|character| self.display_char(*character))
            .collect()
    }

    pub fn current_display(&self) -> Option<char> {
        self.current().map(|character| self.display_char(character))
    }

    pub fn display_value(&self) -> String {
        self.chars
            .iter()
            .map(|character| self.display_char(*character))
            .collect()
    }

    fn display_char(&self, character: char) -> char {
        if self.masked { '•' } else { character }
    }

    /// Handle a key that edits the text. Returns true when the key was used.
    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Char('a') if control => {
                self.home();
                true
            }
            KeyCode::Char('e') if control => {
                self.end();
                true
            }
            KeyCode::Char('u') if control => {
                self.clear();
                true
            }
            KeyCode::Char('w') if control => {
                self.delete_word();
                true
            }
            KeyCode::Char(character) if !control && !key.modifiers.contains(KeyModifiers::ALT) => {
                self.insert(character);
                true
            }
            KeyCode::Backspace => {
                self.backspace();
                true
            }
            KeyCode::Delete => {
                self.delete();
                true
            }
            KeyCode::Left => {
                self.left();
                true
            }
            KeyCode::Right => {
                self.right();
                true
            }
            KeyCode::Home => {
                self.home();
                true
            }
            KeyCode::End => {
                self.end();
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_at_the_cursor() {
        let mut input = TextInput::new("abc");
        input.left();
        input.insert('X');
        assert_eq!(input.value(), "abXc");
        input.backspace();
        assert_eq!(input.value(), "abc");
        input.delete();
        assert_eq!(input.value(), "ab");
        input.home();
        input.insert('>');
        assert_eq!(input.value(), ">ab");
    }

    #[test]
    fn masks_displayed_value() {
        let mut input = TextInput::secret();
        for character in "secret".chars() {
            input.insert(character);
        }
        assert_eq!(input.value(), "secret");
        assert_eq!(input.display_value(), "••••••");
    }

    #[test]
    fn handles_control_shortcuts() {
        let mut input = TextInput::new("one two");
        assert!(input.handle_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL)));
        assert_eq!(input.value(), "one ");
        assert!(input.handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL)));
        assert!(input.is_empty());
    }
}
