use avt::{Color as AvtColor, Vt};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
};
use std::fmt::Write as _;

const SCROLLBACK_LINES: usize = 10_000;

pub struct AgentScreen {
    terminal: Vt,
    scrollback: usize,
    utf8_tail: Vec<u8>,
    modes: InputModes,
}

impl AgentScreen {
    pub fn new(rows: u16, cols: u16) -> Self {
        Self {
            terminal: Vt::builder()
                .size(cols.max(1) as usize, rows.max(1) as usize)
                .scrollback_limit(SCROLLBACK_LINES)
                .build(),
            scrollback: 0,
            utf8_tail: Vec::new(),
            modes: InputModes::default(),
        }
    }

    pub fn process(&mut self, bytes: &[u8]) {
        self.modes.process(bytes);
        self.utf8_tail.extend_from_slice(bytes);
        loop {
            match std::str::from_utf8(&self.utf8_tail) {
                Ok(text) => {
                    self.terminal.feed_str(text);
                    self.utf8_tail.clear();
                    break;
                }
                Err(error) => {
                    let valid = error.valid_up_to();
                    if valid > 0 {
                        let text = std::str::from_utf8(&self.utf8_tail[..valid]).unwrap();
                        self.terminal.feed_str(text);
                        self.utf8_tail.drain(..valid);
                    }
                    if error.error_len().is_some() {
                        self.terminal.feed('\u{fffd}');
                        self.utf8_tail.remove(0);
                    } else {
                        break;
                    }
                }
            }
        }
        self.scrollback = self.scrollback.min(self.history_len());
    }

    pub fn resize(&mut self, rows: u16, cols: u16) {
        let rows = rows.max(1);
        let cols = cols.max(1);
        if cols == 1 && self.terminal.size().0 > 1 {
            let cursor = self.terminal.cursor();
            let mut contents = String::new();
            for (row, line) in self.visible_lines().take(rows as usize).enumerate() {
                if let Some(cell) = line.cells().first().filter(|cell| cell.width() == 1) {
                    let _ = write!(contents, "\x1b[{};1H{}", row + 1, cell.char());
                }
            }
            let _ = write!(
                contents,
                "\x1b[{};1H",
                cursor.row.min(rows as usize - 1) + 1
            );
            self.terminal = Vt::builder()
                .size(1, rows as usize)
                .scrollback_limit(SCROLLBACK_LINES)
                .build();
            self.terminal.feed_str(&contents);
        } else {
            self.terminal.resize(cols as usize, rows as usize);
        }
        self.scrollback = self.scrollback.min(self.history_len());
    }

    pub fn size(&self) -> (u16, u16) {
        let (cols, rows) = self.terminal.size();
        (rows as u16, cols as u16)
    }

    pub fn cursor_position(&self) -> (u16, u16) {
        let cursor = self.terminal.cursor();
        (cursor.row as u16, cursor.col as u16)
    }

    pub fn cursor_visible(&self) -> bool {
        self.scrollback == 0 && self.terminal.cursor().visible
    }

    pub fn captures_mouse(&self) -> bool {
        self.modes.mouse
    }

    pub fn alternate_scroll(&self) -> bool {
        self.modes.alternate_scroll
    }

    pub fn scroll(&mut self, rows: i16) {
        self.scrollback = if rows >= 0 {
            self.scrollback.saturating_add(rows as usize)
        } else {
            self.scrollback.saturating_sub(rows.unsigned_abs() as usize)
        }
        .min(self.history_len());
    }

    pub fn input_mode_formatted(&self) -> Vec<u8> {
        self.modes.formatted()
    }

    #[cfg(windows)]
    pub fn application_cursor(&self) -> bool {
        self.modes.application_cursor
    }

    #[cfg(windows)]
    pub fn bracketed_paste(&self) -> bool {
        self.modes.bracketed_paste
    }

    pub fn render(&self, area: Rect, buffer: &mut Buffer) {
        let (rows, cols) = self.size();
        for (row, line) in self
            .visible_lines()
            .take(rows.min(area.height) as usize)
            .enumerate()
        {
            for (col, source) in line
                .cells()
                .iter()
                .take(cols.min(area.width) as usize)
                .enumerate()
            {
                if source.width() == 0 {
                    continue;
                }
                let Some(cell) = buffer.cell_mut((area.x + col as u16, area.y + row as u16)) else {
                    continue;
                };
                cell.set_char(source.char());
                cell.set_style(cell_style(source));
            }
        }
    }

    fn history_len(&self) -> usize {
        self.terminal
            .lines()
            .count()
            .saturating_sub(self.terminal.size().1)
    }

    fn visible_lines(&self) -> impl Iterator<Item = &avt::Line> {
        let rows = self.terminal.size().1;
        let start = self
            .terminal
            .lines()
            .count()
            .saturating_sub(rows + self.scrollback);
        self.terminal.lines().skip(start).take(rows)
    }

    #[cfg(test)]
    fn text_at(&self, row: u16, col: u16) -> String {
        self.visible_lines()
            .nth(row as usize)
            .map(|line| {
                line.cells()
                    .iter()
                    .skip(col as usize)
                    .take_while(|cell| cell.char() != ' ')
                    .filter(|cell| cell.width() > 0)
                    .map(avt::Cell::char)
                    .collect()
            })
            .unwrap_or_default()
    }
}

fn cell_style(cell: &avt::Cell) -> Style {
    let pen = cell.pen();
    let mut modifiers = Modifier::empty();
    for (enabled, modifier) in [
        (pen.is_bold(), Modifier::BOLD),
        (pen.is_faint(), Modifier::DIM),
        (pen.is_italic(), Modifier::ITALIC),
        (pen.is_underline(), Modifier::UNDERLINED),
        (pen.is_inverse(), Modifier::REVERSED),
    ] {
        if enabled {
            modifiers.insert(modifier);
        }
    }
    Style::default()
        .fg(color(pen.foreground()))
        .bg(color(pen.background()))
        .add_modifier(modifiers)
}

fn color(color: Option<AvtColor>) -> Color {
    match color {
        None => Color::Reset,
        Some(AvtColor::Indexed(index)) => Color::Indexed(index),
        Some(AvtColor::RGB(rgb)) => Color::Rgb(rgb.r, rgb.g, rgb.b),
    }
}

#[derive(Default)]
struct InputModes {
    tail: Vec<u8>,
    application_cursor: bool,
    application_keypad: bool,
    bracketed_paste: bool,
    mouse: bool,
    mouse_sgr: bool,
    alternate_scroll: bool,
}

impl InputModes {
    fn process(&mut self, bytes: &[u8]) {
        self.tail.extend_from_slice(bytes);
        for index in 0..self.tail.len() {
            let remaining = &self.tail[index..];
            if remaining.starts_with(b"\x1b=") {
                self.application_keypad = true;
            } else if remaining.starts_with(b"\x1b>") {
                self.application_keypad = false;
            }
            for (prefix, target) in [
                (b"\x1b[?1".as_slice(), 1),
                (b"\x1b[?2004".as_slice(), 2004),
                (b"\x1b[?1000".as_slice(), 1000),
                (b"\x1b[?1002".as_slice(), 1002),
                (b"\x1b[?1003".as_slice(), 1003),
                (b"\x1b[?1006".as_slice(), 1006),
                (b"\x1b[?1007".as_slice(), 1007),
            ] {
                let Some(enabled) = remaining.get(prefix.len()).and_then(|byte| match byte {
                    b'h' => Some(true),
                    b'l' => Some(false),
                    _ => None,
                }) else {
                    continue;
                };
                match target {
                    1 => self.application_cursor = enabled,
                    2004 => self.bracketed_paste = enabled,
                    1000 | 1002 | 1003 => self.mouse = enabled,
                    1006 => self.mouse_sgr = enabled,
                    1007 => self.alternate_scroll = enabled,
                    _ => unreachable!(),
                }
            }
        }
        if self.tail.len() > 8 {
            self.tail.drain(..self.tail.len() - 8);
        }
    }

    fn formatted(&self) -> Vec<u8> {
        let mut modes = if self.application_keypad {
            b"\x1b=".to_vec()
        } else {
            b"\x1b>".to_vec()
        };
        push_mode(&mut modes, 1, self.application_cursor);
        push_mode(&mut modes, 2004, self.bracketed_paste);
        if self.mouse {
            modes.extend_from_slice(b"\x1b[?1000h");
        }
        if self.mouse_sgr {
            modes.extend_from_slice(b"\x1b[?1006h");
        }
        modes
    }
}

fn push_mode(output: &mut Vec<u8>, mode: u16, enabled: bool) {
    output.extend_from_slice(format!("\x1b[?{mode}{}", if enabled { 'h' } else { 'l' }).as_bytes());
}

#[cfg(test)]
mod tests {
    use super::AgentScreen;
    use ratatui::{
        buffer::Buffer,
        layout::Rect,
        style::{Color, Modifier},
    };

    #[test]
    fn split_alternate_screen_output_stays_inside_the_virtual_screen() {
        let mut screen = AgentScreen::new(3, 12);
        screen.process(b"main\x1b[?10");
        screen.process(b"49halt\x1b[2;3H!\x1b[?1049l");
        assert_eq!(screen.text_at(0, 0), "main");
        assert_eq!(screen.cursor_position(), (0, 4));
    }

    #[test]
    fn shrinking_after_a_wide_cell_does_not_panic() {
        let mut screen = AgentScreen::new(2, 4);
        screen.process("你".as_bytes());
        screen.resize(2, 1);
        screen.process(b"\x1b[K");
        assert_eq!(screen.size(), (2, 1));
    }

    #[test]
    fn render_preserves_terminal_cell_style() {
        let mut screen = AgentScreen::new(1, 2);
        screen.process(b"\x1b[31;44;1;3;4;7mX");
        let mut buffer = Buffer::empty(Rect::new(0, 0, 2, 1));
        screen.render(buffer.area, &mut buffer);
        let cell = &buffer[(0, 0)];
        assert_eq!(cell.symbol(), "X");
        assert_eq!(cell.fg, Color::Indexed(1));
        assert_eq!(cell.bg, Color::Indexed(4));
        assert!(cell.modifier.contains(Modifier::BOLD));
        assert!(cell.modifier.contains(Modifier::ITALIC));
        assert!(cell.modifier.contains(Modifier::UNDERLINED));
        assert!(cell.modifier.contains(Modifier::REVERSED));
    }

    #[test]
    fn mirrors_agent_mouse_modes_to_the_real_terminal() {
        let mut screen = AgentScreen::new(1, 1);
        assert!(!screen.captures_mouse());
        screen.process(b"\x1b[?1000h\x1b[?1006h");
        assert!(screen.captures_mouse());
        let modes = screen.input_mode_formatted();
        assert!(modes.windows(8).any(|mode| mode == b"\x1b[?1000h"));
        assert!(modes.windows(8).any(|mode| mode == b"\x1b[?1006h"));
    }

    #[test]
    fn tracks_fragmented_xterm_alternate_scroll_mode() {
        let mut screen = AgentScreen::new(1, 1);
        screen.process(b"\x1b[?10");
        screen.process(b"07h");
        assert!(screen.alternate_scroll());
        screen.process(b"\x1b[?1007l");
        assert!(!screen.alternate_scroll());
    }

    #[test]
    fn retains_scrolled_lines_for_wrapper_scrollback() {
        let mut screen = AgentScreen::new(2, 8);
        screen.process(b"one\r\ntwo\r\nthree");
        screen.scroll(1);
        assert_eq!(screen.text_at(0, 0), "one");
    }

    #[test]
    fn retains_history_from_top_anchored_partial_scroll_region() {
        let mut screen = AgentScreen::new(3, 8);
        screen.process(b"one\r\ntwo\x1b[1;2r\x1b[2;1H\r\nthree\x1b[r");
        screen.scroll(1);
        assert_eq!(screen.text_at(0, 0), "one");
    }
}
