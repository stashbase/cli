use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
};
use std::fmt::Write as _;

pub struct AgentScreen {
    parser: vt100::Parser,
}

impl AgentScreen {
    pub fn new(rows: u16, cols: u16) -> Self {
        Self {
            parser: vt100::Parser::new(rows.max(1), cols.max(1), 0),
        }
    }

    pub fn process(&mut self, bytes: &[u8]) {
        self.parser.process(bytes);
    }

    pub fn resize(&mut self, rows: u16, cols: u16) {
        let rows = rows.max(1);
        let cols = cols.max(1);
        let (_, old_cols) = self.size();
        if cols < old_cols {
            self.rebuild(rows, cols);
        } else {
            self.parser.screen_mut().set_size(rows, cols);
        }
    }

    pub fn size(&self) -> (u16, u16) {
        self.parser.screen().size()
    }

    pub fn cursor_position(&self) -> (u16, u16) {
        self.parser.screen().cursor_position()
    }

    pub fn cursor_visible(&self) -> bool {
        !self.parser.screen().hide_cursor()
    }

    pub fn input_mode_formatted(&self) -> Vec<u8> {
        self.parser.screen().input_mode_formatted()
    }

    #[cfg(windows)]
    pub fn application_cursor(&self) -> bool {
        self.parser.screen().application_cursor()
    }

    #[cfg(windows)]
    pub fn bracketed_paste(&self) -> bool {
        self.parser.screen().bracketed_paste()
    }

    pub fn render(&self, area: Rect, buffer: &mut Buffer) {
        let screen = self.parser.screen();
        let (rows, cols) = screen.size();
        for row in 0..rows.min(area.height) {
            for col in 0..cols.min(area.width) {
                let Some(source) = screen.cell(row, col) else {
                    continue;
                };
                if source.is_wide_continuation() {
                    continue;
                }
                let Some(cell) = buffer.cell_mut((area.x + col, area.y + row)) else {
                    continue;
                };
                cell.set_symbol(if source.has_contents() {
                    source.contents()
                } else {
                    " "
                });
                cell.set_style(cell_style(source));
            }
        }
    }

    fn rebuild(&mut self, rows: u16, cols: u16) {
        let screen = self.parser.screen();
        let was_alternate = screen.alternate_screen();
        let cursor_hidden = screen.hide_cursor();
        let (cursor_row, cursor_col) = screen.cursor_position();
        let (old_rows, old_cols) = screen.size();
        let mut contents = String::new();
        if was_alternate {
            contents.push_str("\x1b[?1049h");
        }
        for row in 0..old_rows.min(rows) {
            for col in 0..old_cols.min(cols) {
                let Some(cell) = screen.cell(row, col) else {
                    continue;
                };
                if !cell.has_contents()
                    || cell.is_wide_continuation()
                    || (cell.is_wide() && col + 1 >= cols)
                {
                    continue;
                }
                let _ = write!(contents, "\x1b[{};{}H{}", row + 1, col + 1, cell.contents());
            }
        }
        let _ = write!(
            contents,
            "\x1b[{};{}H",
            cursor_row.min(rows - 1) + 1,
            cursor_col.min(cols - 1) + 1
        );
        if cursor_hidden {
            contents.push_str("\x1b[?25l");
        }
        let mut parser = vt100::Parser::new(rows, cols, 0);
        parser.process(contents.as_bytes());
        self.parser = parser;
    }

    #[cfg(test)]
    fn text_at(&self, row: u16, col: u16) -> String {
        let screen = self.parser.screen();
        let (_, cols) = screen.size();
        (col..cols)
            .filter_map(|column| screen.cell(row, column))
            .take_while(|cell| cell.has_contents())
            .filter(|cell| !cell.is_wide_continuation())
            .map(vt100::Cell::contents)
            .collect()
    }
}

fn cell_style(cell: &vt100::Cell) -> Style {
    let mut modifiers = Modifier::empty();
    for (enabled, modifier) in [
        (cell.bold(), Modifier::BOLD),
        (cell.dim(), Modifier::DIM),
        (cell.italic(), Modifier::ITALIC),
        (cell.underline(), Modifier::UNDERLINED),
        (cell.inverse(), Modifier::REVERSED),
    ] {
        if enabled {
            modifiers.insert(modifier);
        }
    }
    Style::default()
        .fg(color(cell.fgcolor()))
        .bg(color(cell.bgcolor()))
        .add_modifier(modifiers)
}

fn color(color: vt100::Color) -> Color {
    match color {
        vt100::Color::Default => Color::Reset,
        vt100::Color::Idx(index) => Color::Indexed(index),
        vt100::Color::Rgb(red, green, blue) => Color::Rgb(red, green, blue),
    }
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
}
