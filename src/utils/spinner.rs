use std::borrow::Cow;
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use colored::Colorize;

use super::term_size::get_terminal_size;

#[derive(Debug, Clone, Copy, Default)]
pub enum Streams {
    #[default]
    Stdout,
    Stderr,
}

impl Streams {
    // Every write goes out as one buffered `write_all` + `flush`: stderr is
    // unbuffered, so writing a frame piecewise lets the terminal repaint a
    // half-cleared line in between, which shows up as flicker.
    fn write_atomic(self, bytes: &[u8]) {
        let _ = match self {
            Self::Stdout => {
                let mut stream = io::stdout().lock();
                stream.write_all(bytes).and_then(|()| stream.flush())
            }
            Self::Stderr => {
                let mut stream = io::stderr().lock();
                stream.write_all(bytes).and_then(|()| stream.flush())
            }
        };
    }
}

struct SpinnerFrames {
    frames: &'static [&'static str],
    interval: u64,
}

fn spinner_frames() -> SpinnerFrames {
    #[cfg(windows)]
    {
        SpinnerFrames {
            frames: &["-", "\\", "|", "/"],
            interval: 130,
        }
    }

    #[cfg(not(windows))]
    {
        SpinnerFrames {
            frames: &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"],
            interval: 80,
        }
    }
}

/// Fits `"{frame} {message}"` on one terminal row: a line that wraps can't
/// be returned to with `\r`, so every redraw would leave a stale row behind.
fn fit_message(message: &str, frame_width: usize, terminal_width: usize) -> Cow<'_, str> {
    // Leave the last column free; writing into it makes some terminals wrap.
    let available = terminal_width.saturating_sub(frame_width + 2);
    if message.chars().count() <= available {
        return Cow::Borrowed(message);
    }
    if available == 0 {
        return Cow::Borrowed("");
    }
    let mut fitted: String = message.chars().take(available - 1).collect();
    fitted.push('…');
    Cow::Owned(fitted)
}

/// Renders one frame as a single buffer: return to column 0, draw the frame,
/// then pad over whatever the previous draw left beyond it. The line is
/// never blank between frames. Widths are in visible columns, so the color
/// codes in `painted_frame` don't count.
fn render_frame(
    painted_frame: &str,
    frame_width: usize,
    message: &str,
    previous_width: usize,
) -> (String, usize) {
    let width = frame_width + 1 + message.chars().count();
    let padding = previous_width.saturating_sub(width);
    let line = format!("\r{painted_frame} {message}{:padding$}", "");
    (line, width)
}

fn render_clear(previous_width: usize) -> String {
    format!("\r{:previous_width$}\r", "")
}

pub struct Spinner {
    thread_handle: Option<JoinHandle<()>>,
    still_spinning: Arc<AtomicBool>,
    stream: Streams,
}

impl Spinner {
    fn start(message: Cow<'static, str>, stream: Streams) -> Self {
        let still_spinning = Arc::new(AtomicBool::new(true));
        let thread_handle = thread::spawn({
            let still_spinning = Arc::clone(&still_spinning);
            move || {
                let frames = spinner_frames();
                let frame_width = frames.frames[0].chars().count();
                let (terminal_width, _) = get_terminal_size();
                let message = fit_message(&message, frame_width, terminal_width);
                let mut drawn_width = 0;
                for frame in frames.frames.iter().cycle() {
                    if !still_spinning.load(Ordering::Relaxed) {
                        break;
                    }
                    let painted_frame = frame.cyan().to_string();
                    let (line, width) =
                        render_frame(&painted_frame, frame_width, &message, drawn_width);
                    stream.write_atomic(line.as_bytes());
                    drawn_width = width;
                    thread::sleep(Duration::from_millis(frames.interval));
                }
                stream.write_atomic(render_clear(drawn_width).as_bytes());
            }
        });

        Self {
            thread_handle: Some(thread_handle),
            still_spinning,
            stream,
        }
    }

    /// Stops the animation and erases its line. Safe to call more than once.
    pub fn clear(&mut self) {
        self.still_spinning.store(false, Ordering::Relaxed);
        if let Some(handle) = self.thread_handle.take() {
            let _ = handle.join();
        }
    }

    pub fn stop_with_message(&mut self, msg: &str) {
        self.clear();
        self.stream.write_atomic(format!("{msg}\n").as_bytes());
    }

    pub fn stop_and_persist(&mut self, symbol: &str, msg: &str) {
        self.clear();
        self.stream
            .write_atomic(format!("{symbol} {msg}\n").as_bytes());
    }
}

impl Drop for Spinner {
    fn drop(&mut self) {
        self.clear();
    }
}

pub fn new_spinner(message: impl Into<Cow<'static, str>>, stream: Streams) -> Spinner {
    Spinner::start(message.into(), stream)
}

pub fn request_spinner() -> Spinner {
    new_spinner("Request in progress...", Streams::Stderr)
}

#[cfg(test)]
mod tests {
    use super::{fit_message, render_clear, render_frame, spinner_frames};

    #[cfg(windows)]
    #[test]
    fn uses_ascii_spinner_frames_on_windows() {
        let frames = spinner_frames();
        assert_eq!(frames.frames, ["-", "\\", "|", "/"]);
        assert_eq!(frames.interval, 130);
    }

    #[cfg(not(windows))]
    #[test]
    fn uses_unicode_spinner_frames_on_non_windows() {
        let frames = spinner_frames();
        assert_eq!(
            frames.frames,
            ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]
        );
        assert_eq!(frames.interval, 80);
    }

    #[test]
    fn frame_redraws_in_place_without_blanking_the_line() {
        let (line, width) = render_frame("⠋", 1, "Loading...", 0);
        assert_eq!(line, "\r⠋ Loading...");
        assert_eq!(width, 12);
    }

    #[test]
    fn frame_pads_over_a_longer_previous_draw() {
        let (line, _) = render_frame("⠋", 1, "Hi", 6);
        assert_eq!(line, "\r⠋ Hi  ");
    }

    #[test]
    fn frame_width_ignores_color_codes() {
        let (_, width) = render_frame("\x1b[36m⠋\x1b[0m", 1, "Loading...", 0);
        assert_eq!(width, 12);
    }

    #[test]
    fn clear_erases_exactly_the_drawn_width() {
        assert_eq!(render_clear(4), "\r    \r");
    }

    #[test]
    fn message_that_fits_is_left_alone() {
        assert_eq!(fit_message("Loading...", 1, 80), "Loading...");
    }

    #[test]
    fn message_is_truncated_to_one_row() {
        let fitted = fit_message("Starting network namespace holder and firewall...", 1, 20);
        assert_eq!(fitted, "Starting network…");
        assert_eq!(1 + 1 + fitted.chars().count(), 19);
    }
}
