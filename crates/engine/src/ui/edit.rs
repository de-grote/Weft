//! Text editing with the usual desktop keys: Shift extends a selection, Ctrl moves and deletes by
//! words, Ctrl+A/C/X/V, held keys repeat. Positions are char indices. A field of several lines
//! (`lines`) also takes Enter and Tab, and goes up and down a line.

use winit::keyboard::KeyCode;

use crate::input::{Input, Key};

pub struct Field<'a> {
    pub value: &'a mut String,
    pub caret: &'a mut usize,
    /// Other end of the selection, if any.
    pub anchor: &'a mut Option<usize>,
    pub max: usize,
    pub lines: bool,
}

/// What a key did: the text changed, or only the caret or selection moved.
#[derive(Default, PartialEq, Debug)]
pub struct Outcome {
    pub changed: bool,
    pub moved: bool,
}

fn class(c: char) -> u8 {
    if c.is_whitespace() {
        0
    } else if c.is_alphanumeric() || c == '_' {
        1
    } else {
        2
    }
}

/// Start of the word before `at`, skipping spaces first.
pub fn word_left(chars: &[char], mut at: usize) -> usize {
    while at > 0 && class(chars[at - 1]) == 0 {
        at -= 1;
    }
    if at > 0 {
        let k = class(chars[at - 1]);
        while at > 0 && class(chars[at - 1]) == k {
            at -= 1;
        }
    }
    at
}

/// Start of the next word after `at`: past the current word, then past spaces.
pub fn word_right(chars: &[char], mut at: usize) -> usize {
    if at < chars.len() {
        let k = class(chars[at]);
        while at < chars.len() && class(chars[at]) == k && k != 0 {
            at += 1;
        }
    }
    while at < chars.len() && class(chars[at]) == 0 {
        at += 1;
    }
    at
}

impl Field<'_> {
    fn chars(&self) -> Vec<char> {
        self.value.chars().collect()
    }

    pub fn selection(&self) -> Option<(usize, usize)> {
        self.anchor
            .filter(|&a| a != *self.caret)
            .map(|a| (a.min(*self.caret), a.max(*self.caret)))
    }

    fn set(&mut self, chars: Vec<char>) {
        *self.value = chars.into_iter().collect();
    }

    fn delete_range(&mut self, from: usize, to: usize) {
        let mut chars = self.chars();
        chars.drain(from..to);
        self.set(chars);
        *self.caret = from;
        *self.anchor = None;
    }

    fn delete_selection(&mut self) -> bool {
        match self.selection() {
            Some((a, b)) => {
                self.delete_range(a, b);
                true
            }
            None => false,
        }
    }

    pub fn insert(&mut self, text: &str) -> bool {
        let mut changed = self.delete_selection();
        let mut chars = self.chars();
        let text = if self.lines { text.replace("\r\n", "\n").replace('\t', "    ") } else { text.to_string() };
        for ch in text
            .chars()
            .filter(|&c| !c.is_control() || (self.lines && c == '\n'))
        {
            if chars.len() >= self.max {
                break;
            }
            chars.insert(*self.caret, ch);
            *self.caret += 1;
            changed = true;
        }
        self.set(chars);
        changed
    }

    /// Moves the caret, keeping or starting a selection when `select`.
    fn go(&mut self, to: usize, select: bool) {
        if select {
            self.anchor.get_or_insert(*self.caret);
        } else {
            *self.anchor = None;
        }
        *self.caret = to;
    }

    /// Applies this frame's keys and typed text.
    pub fn handle(&mut self, input: &Input) -> Outcome {
        let mut out = Outcome::default();
        let held = |k: KeyCode| input.held_key(Key::Code(k));
        let ctrl = held(KeyCode::ControlLeft) || held(KeyCode::ControlRight);
        let shift = held(KeyCode::ShiftLeft) || held(KeyCode::ShiftRight);
        let key = |k: KeyCode| input.pressed_or_repeated(Key::Code(k));
        let chars = self.chars();
        let len = chars.len();
        *self.caret = (*self.caret).min(len);
        *self.anchor = self.anchor.map(|a| a.min(len));

        if ctrl && key(KeyCode::KeyA) {
            *self.anchor = Some(0);
            *self.caret = len;
            out.moved = true;
        } else if ctrl && (key(KeyCode::KeyC) || key(KeyCode::KeyX)) {
            if let Some((a, b)) = self.selection() {
                crate::clipboard::set(&chars[a..b].iter().collect::<String>());
                if key(KeyCode::KeyX) {
                    self.delete_range(a, b);
                    out.changed = true;
                }
            }
        } else if ctrl && key(KeyCode::KeyV) {
            if let Some(clip) = crate::clipboard::get() {
                let clip = if self.lines { clip.as_str() } else { clip.lines().next().unwrap_or("") };
                out.changed |= self.insert(clip);
            }
        } else if !input.typed.is_empty() && !ctrl {
            // Tab comes as a key too, which puts the indent in below; typed, it would twice.
            out.changed |= self.insert(&input.typed.replace('\t', ""));
        }
        if self.lines && key(KeyCode::Enter) {
            out.changed |= self.insert("\n");
        }
        if self.lines && key(KeyCode::Tab) {
            out.changed |= self.insert("    ");
        }

        if key(KeyCode::Backspace) {
            if self.delete_selection() {
                out.changed = true;
            } else if *self.caret > 0 {
                let chars = self.chars();
                let from = if ctrl { word_left(&chars, *self.caret) } else { *self.caret - 1 };
                let to = *self.caret;
                self.delete_range(from, to);
                out.changed = true;
            }
        }
        if key(KeyCode::Delete) {
            let chars = self.chars();
            if self.delete_selection() {
                out.changed = true;
            } else if *self.caret < chars.len() {
                let at = *self.caret;
                let to = if ctrl { word_right(&chars, at) } else { at + 1 };
                self.delete_range(at, to);
                out.changed = true;
            }
        }

        let chars = self.chars();
        let len = chars.len();
        let caret = *self.caret;
        let target = if key(KeyCode::ArrowLeft) {
            Some(match (ctrl, self.selection()) {
                (true, _) => word_left(&chars, caret),
                (false, Some((a, _))) if !shift => a,
                _ => caret.saturating_sub(1),
            })
        } else if key(KeyCode::ArrowRight) {
            Some(match (ctrl, self.selection()) {
                (true, _) => word_right(&chars, caret),
                (false, Some((_, b))) if !shift => b,
                _ => (caret + 1).min(len),
            })
        } else if self.lines && !ctrl && (key(KeyCode::Home) || key(KeyCode::End)) {
            // The start or end of the line the caret is on.
            let start = chars[..caret]
                .iter()
                .rposition(|&c| c == '\n')
                .map_or(0, |i| i + 1);
            let end = chars[caret..]
                .iter()
                .position(|&c| c == '\n')
                .map_or(len, |i| caret + i);
            Some(if key(KeyCode::Home) { start } else { end })
        } else if self.lines && (key(KeyCode::ArrowUp) || key(KeyCode::ArrowDown)) {
            Some(line_step(&chars, caret, key(KeyCode::ArrowDown)))
        } else if key(KeyCode::Home) {
            Some(0)
        } else if key(KeyCode::End) {
            Some(len)
        } else {
            None
        };
        if let Some(t) = target {
            self.go(t, shift);
            out.moved = true;
        }
        out
    }
}

/// The caret a line up or down, in the same column or at that line's end if it is shorter.
fn line_step(chars: &[char], caret: usize, down: bool) -> usize {
    let start = chars[..caret]
        .iter()
        .rposition(|&c| c == '\n')
        .map_or(0, |i| i + 1);
    let col = caret - start;
    let (from, to) = if down {
        let Some(end) = chars[caret..]
            .iter()
            .position(|&c| c == '\n')
            .map(|i| caret + i)
        else {
            return chars.len();
        };
        let next = end + 1;
        (
            next,
            chars[next..]
                .iter()
                .position(|&c| c == '\n')
                .map_or(chars.len(), |i| next + i),
        )
    } else {
        if start == 0 {
            return 0;
        }
        let prev = chars[..start - 1]
            .iter()
            .rposition(|&c| c == '\n')
            .map_or(0, |i| i + 1);
        (prev, start - 1)
    };
    (from + col).min(to)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_motions() {
        let c: Vec<char> = "/tp  @e[type=zombie] ~ ~3".chars().collect();
        assert_eq!(word_left(&c, c.len()), c.len() - 1);
        assert_eq!(word_left(&c, 5), 1);
        assert_eq!(word_right(&c, 0), 1);
        assert_eq!(word_right(&c, 1), 5);
    }

    #[test]
    fn typing_replaces_the_selection() {
        let (mut v, mut caret, mut anchor) = ("hello world".to_string(), 11, Some(6));
        let mut f = Field { value: &mut v, caret: &mut caret, anchor: &mut anchor, max: 32, lines: false };
        assert!(f.insert("there"));
        assert_eq!(v, "hello there");
        assert_eq!((caret, anchor), (11, None));
    }

    #[test]
    fn lines_up_and_down() {
        let c: Vec<char> = "abcd\nx\nlonger".chars().collect();
        assert_eq!(line_step(&c, 3, true), 6);
        assert_eq!(line_step(&c, 6, true), 8);
        assert_eq!(line_step(&c, 9, false), 6);
        assert_eq!(line_step(&c, 6, false), 1);
        assert_eq!(line_step(&c, 2, false), 0);
    }
}
