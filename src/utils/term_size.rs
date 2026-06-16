use std::env;

use terminal_size::{terminal_size, Height as TerminalHeight, Width as TerminalWidth};

const DEFAULT_TERMINAL_WIDTH: usize = 120;
const DEFAULT_TERMINAL_HEIGHT: usize = 40;

pub fn get_terminal_size() -> (usize, usize) {
    if let Some((TerminalWidth(width), TerminalHeight(height))) = terminal_size() {
        return (width as usize, height as usize);
    }

    let width = env::var("COLUMNS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_TERMINAL_WIDTH);

    let height = env::var("LINES")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_TERMINAL_HEIGHT);

    (width, height)
}
