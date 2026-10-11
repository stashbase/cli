//! Optional PTY-aware terminal wrapper for `stashbase agent run --tui`.
//!
//! This is a passive frame, not a dashboard: the child agent gets its own
//! pseudo-terminal sized to leave room for a bottom status bar, and every
//! keystroke, resize, and signal is forwarded to it exactly as a plain
//! terminal would. Stashbase only owns the bottom status lines. The status
//! bar renders counts and identifiers only; it must never be handed a secret
//! value, a request body, a header, or a URL.

use std::collections::HashMap;
use std::ffi::OsString;
use std::io::IsTerminal;
use std::path::PathBuf;
use std::time::{Duration, Instant};

mod pty;
mod screen;

#[derive(Debug)]
pub struct TuiCommand {
    pub program: OsString,
    pub args: Vec<OsString>,
    pub cwd: PathBuf,
    pub env: Vec<(OsString, OsString)>,
    pub env_removals: Vec<OsString>,
}

impl TuiCommand {
    pub fn into_builder(self) -> portable_pty::CommandBuilder {
        let mut builder = portable_pty::CommandBuilder::new(self.program);
        builder.args(self.args);
        builder.cwd(self.cwd);
        for name in self.env_removals {
            builder.env_remove(name);
        }
        for (name, value) in self.env {
            builder.env(name, value);
        }
        builder
    }
}

/// Most lines the status bar's content is allowed to wrap onto, not counting
/// the separator line above it. Content that still doesn't fit in this many
/// lines is dropped rather than growing the bar further.
pub const MAX_CONTENT_LINES: u16 = 3;
const CONTENT_PADDING: usize = 1;

/// Smallest terminal we are willing to reshape for `--tui`. Below this the
/// status bar would crowd out the agent's own UI, so we fail instead of
/// producing a corrupted layout.
pub const MIN_ROWS: u16 = 8;
pub const MIN_COLS: u16 = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TuiMode {
    Local,
    Remote,
}

impl TuiMode {
    pub fn label(&self) -> &'static str {
        match self {
            TuiMode::Local => "local",
            TuiMode::Remote => "remote",
        }
    }
}

/// Everything the status bar is allowed to display. Every field here is
/// either a count, a name, or an identifier that is already surfaced
/// elsewhere by `agent run` (profile name, session ID); none of it requires
/// fetching or holding a secret value.
#[derive(Debug, Clone)]
pub struct TuiStatusInfo {
    pub profile: String,
    pub global_profile: bool,
    pub mode: TuiMode,
    /// Set once the run has settled on the Docker sandbox backend.
    pub docker_sandbox: bool,
    pub session_id: Option<String>,
    pub egress_host_count: usize,
    pub filesystem_read_denial_count: usize,
    pub filesystem_write_denial_count: usize,
    pub shared_secret_count: usize,
    pub personal_credential_count: usize,
    pub mcp_server_count: usize,
    /// `None` when the profile does not restrict every configured MCP server
    /// to an explicit tool allowlist, so no single bounded count applies.
    pub mcp_allowed_tool_count: Option<usize>,
    pub project_environment: Option<String>,
}

impl TuiStatusInfo {
    /// Sums `allow_tools` across configured MCP servers when every server
    /// declares an explicit allowlist; otherwise the count is unbounded and
    /// this returns `None` rather than a misleading number.
    pub fn mcp_allowed_tool_count_from(
        mcp_servers: &HashMap<String, crate::models::agent::AgentMcpServer>,
    ) -> Option<usize> {
        if mcp_servers.is_empty() {
            return None;
        }
        if mcp_servers
            .values()
            .any(|server| server.allow_tools.is_empty())
        {
            return None;
        }
        Some(
            mcp_servers
                .values()
                .map(|server| server.allow_tools.len())
                .sum(),
        )
    }
}

/// Identity chips: session (when known), profile, mode, sandbox backend, and
/// project/environment.
fn identity_chips(info: &TuiStatusInfo) -> Vec<String> {
    let mut chips = Vec::new();
    if let Some(session_id) = &info.session_id {
        chips.push(session_id.clone());
    }
    chips.push(info.profile.clone());
    if info.global_profile {
        chips.push("global".to_owned());
    }
    chips.push(info.mode.label().to_owned());
    if info.docker_sandbox {
        chips.push("docker".to_owned());
    }
    if let Some(project_environment) = &info.project_environment {
        chips.push(project_environment.clone());
    }
    chips
}

/// Metrics chips: counts only, never a secret name or value.
fn metrics_chips(info: &TuiStatusInfo) -> Vec<String> {
    let mut chips = vec![format!(
        "Egress: {} host{}",
        info.egress_host_count,
        if info.egress_host_count == 1 { "" } else { "s" }
    )];
    if info.shared_secret_count > 0 || info.personal_credential_count > 0 {
        let mut bindings = Vec::new();
        if info.shared_secret_count > 0 {
            bindings.push(format!("{} secrets", info.shared_secret_count));
        }
        if info.personal_credential_count > 0 {
            bindings.push(format!("{} personal", info.personal_credential_count));
        }
        chips.push(format!("Bindings: {}", bindings.join(", ")));
    }
    if info.mcp_server_count > 0 {
        chips.push(format!(
            "MCP: {}",
            match info.mcp_allowed_tool_count {
                Some(count) => format!("{count} allowed"),
                None => "unrestricted".to_owned(),
            }
        ));
    }
    if info.filesystem_read_denial_count > 0 || info.filesystem_write_denial_count > 0 {
        let mut rules = Vec::new();
        if info.filesystem_read_denial_count > 0 {
            rules.push(format!("{} read", info.filesystem_read_denial_count));
        }
        if info.filesystem_write_denial_count > 0 {
            rules.push(format!("{} write", info.filesystem_write_denial_count));
        }
        chips.insert(0, format!("Filesystem: {} denied", rules.join(", ")));
    }
    chips
}

/// Greedily packs `chips` onto as few lines as fit in `cols`, wrapping onto a
/// new line (rather than truncating) whenever the next chip would overflow
/// the current one, up to `max_lines`. A single chip wider than `cols` is
/// truncated with an ellipsis; content beyond `max_lines` is dropped.
fn wrap_chips(chips: &[String], cols: usize, max_lines: usize) -> Vec<String> {
    let cols = cols.max(1);
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    for raw_chip in chips {
        let chip = if raw_chip.chars().count() > cols {
            truncate(raw_chip, cols)
        } else {
            raw_chip.clone()
        };
        let candidate = if current.is_empty() {
            chip.clone()
        } else {
            format!("{current} · {chip}")
        };
        if candidate.chars().count() <= cols {
            current = candidate;
            continue;
        }
        if !current.is_empty() {
            lines.push(std::mem::take(&mut current));
        }
        if lines.len() >= max_lines {
            break;
        }
        current = chip;
    }
    if !current.is_empty() && lines.len() < max_lines {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines.truncate(max_lines);
    lines
}

fn truncate(line: &str, cols: usize) -> String {
    if line.chars().count() <= cols {
        return line.to_owned();
    }
    if cols == 0 {
        return String::new();
    }
    let mut truncated: String = line.chars().take(cols.saturating_sub(1)).collect();
    truncated.push('…');
    truncated
}

/// How many content lines (not counting the separator) the bar needs for
/// `cols`. Uses `WIDEST_STATUS_LABEL` so the result stays the same across a
/// state transition and only changes when `cols` actually changes.
pub fn content_line_count(cols: u16, info: &TuiStatusInfo) -> u16 {
    let mut chips = identity_chips(info);
    chips.extend(metrics_chips(info));
    wrap_chips(
        &chips,
        (cols as usize).saturating_sub(CONTENT_PADDING * 2),
        MAX_CONTENT_LINES as usize,
    )
    .len() as u16
}

/// A frame consisting of a plain separator line followed by the bar's
/// content, wrapped onto as many lines as it needs (up to
/// `MAX_CONTENT_LINES`). Returns the frame text and the total number of
/// rows it occupies, including the separator, so the caller can reserve
/// exactly that much of the real terminal.
pub fn render_frame(cols: u16, info: &TuiStatusInfo) -> (String, u16) {
    let reserved_lines = content_line_count(cols, info);
    let mut chips = identity_chips(info);
    chips.extend(metrics_chips(info));
    let mut lines = wrap_chips(
        &chips,
        (cols as usize).saturating_sub(CONTENT_PADDING * 2),
        MAX_CONTENT_LINES as usize,
    );
    while (lines.len() as u16) < reserved_lines {
        lines.push(String::new());
    }
    let separator = "─".repeat((cols as usize).max(MIN_COLS as usize));
    let mut frame_lines = Vec::with_capacity(1 + lines.len());
    frame_lines.push(separator);
    frame_lines.extend(lines.into_iter().map(|line| format!(" {line}")));
    (frame_lines.join("\n"), 1 + reserved_lines)
}

const FRAME_INTERVAL: Duration = Duration::from_millis(16);
// 1002 (button-event tracking) adds drag motion, for wrapper-side selection.
const MOUSE_CAPTURE_ENABLE: &[u8] = b"\x1b[?1000h\x1b[?1002h\x1b[?1006h";
const MOUSE_CAPTURE_DISABLE: &[u8] =
    b"\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1005l\x1b[?1006l\x1b[?1015l";
const TERMINAL_MODE_RESET: &[u8] = b"\x1b[?1l\x1b>\x1b[?2004l\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1005l\x1b[?1006l\x1b[?1015l\x1b[?2026l";

fn physical_input_modes(child_modes: &[u8], child_captures_mouse: bool) -> Vec<u8> {
    let mut modes = MOUSE_CAPTURE_DISABLE.to_vec();
    modes.extend_from_slice(child_modes);
    if !child_captures_mouse {
        modes.extend_from_slice(MOUSE_CAPTURE_ENABLE);
    }
    modes
}

#[derive(Clone, Copy)]
enum WheelRouting {
    Wrapper,
    AlternateScroll,
    Child,
}

/// What the input thread asks the render loop to do with mouse input the
/// wrapper keeps for itself. Positions are 0-based in the real terminal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MouseAction {
    Scroll(i16),
    Press {
        row: u16,
        col: u16,
    },
    Drag {
        row: u16,
        col: u16,
    },
    Release {
        row: u16,
        col: u16,
    },
    /// The user typed into the agent: drop any highlighted selection.
    ClearSelection,
}

/// Copies `text` to the clipboard of the terminal the user is looking at
/// (OSC 52, which also works over SSH) and, on macOS, to the local
/// pasteboard for terminals that ignore OSC 52.
fn copy_to_clipboard(text: &str) {
    use base64::Engine as _;
    use std::io::Write as _;
    let mut stdout = std::io::stdout();
    let _ = write!(
        stdout,
        "\x1b]52;c;{}\x07",
        base64::engine::general_purpose::STANDARD.encode(text)
    );
    let _ = stdout.flush();
    #[cfg(target_os = "macos")]
    {
        let text = text.to_owned();
        std::thread::spawn(move || {
            if let Ok(mut child) = std::process::Command::new("pbcopy")
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
            {
                if let Some(mut stdin) = child.stdin.take() {
                    let _ = stdin.write_all(text.as_bytes());
                }
                let _ = child.wait();
            }
        });
    }
}

struct TuiState {
    screen: screen::AgentScreen,
    child_rows: u16,
    cols: u16,
    dirty: bool,
    next_draw: Instant,
    input_modes: Vec<u8>,
}

impl TuiState {
    fn new(child_rows: u16, cols: u16, now: Instant) -> Self {
        Self {
            screen: screen::AgentScreen::new(child_rows, cols),
            child_rows,
            cols,
            dirty: true,
            next_draw: now,
            input_modes: Vec::new(),
        }
    }

    fn output(&mut self, bytes: &[u8]) {
        self.screen.process(bytes);
        self.dirty = true;
    }

    fn take_input_mode_update(&mut self) -> Option<Vec<u8>> {
        let modes = self.screen.input_mode_formatted();
        if modes == self.input_modes {
            None
        } else {
            self.input_modes = modes.clone();
            Some(modes)
        }
    }

    fn resize(&mut self, child_rows: u16, cols: u16) {
        self.child_rows = child_rows;
        self.cols = cols;
        self.screen.resize(child_rows, cols);
        self.dirty = true;
    }

    fn mouse(&mut self, action: MouseAction) {
        match action {
            MouseAction::Scroll(rows) => self.screen.scroll(rows),
            MouseAction::Press { row, col } if row < self.child_rows => self
                .screen
                .start_selection(row, col.min(self.cols.saturating_sub(1))),
            MouseAction::Press { .. } => self.screen.clear_selection(),
            MouseAction::Drag { row, col } => self.screen.extend_selection(
                row.min(self.child_rows.saturating_sub(1)),
                col.min(self.cols.saturating_sub(1)),
            ),
            MouseAction::Release { row, col } => {
                if self.screen.has_selection() {
                    self.screen.extend_selection(
                        row.min(self.child_rows.saturating_sub(1)),
                        col.min(self.cols.saturating_sub(1)),
                    );
                    if let Some(text) = self.screen.finish_selection() {
                        copy_to_clipboard(&text);
                    }
                }
            }
            MouseAction::ClearSelection => {
                if !self.screen.has_selection() {
                    return;
                }
                self.screen.clear_selection();
            }
        }
        self.dirty = true;
    }

    fn child_captures_mouse(&self) -> bool {
        self.screen.captures_mouse()
    }

    fn child_uses_alternate_scroll(&self) -> bool {
        self.screen.alternate_scroll()
    }

    fn should_draw(&self, now: Instant) -> bool {
        self.dirty && now >= self.next_draw
    }

    fn did_draw(&mut self, now: Instant) {
        self.dirty = false;
        self.next_draw = now + FRAME_INTERVAL;
    }
}

pub fn tui_supported() -> bool {
    std::io::stdin().is_terminal() && std::io::stdout().is_terminal()
}

pub fn tui_unsupported_reason() -> &'static str {
    "--tui requires an interactive terminal on both stdin and stdout"
}

pub fn forward_signal_to_tui_child(_signal: i32) {
    pty::interrupt_active_child();
}

pub use imp::run_command_in_tui;

mod imp {
    use super::{pty, TuiState, TuiStatusInfo, FRAME_INTERVAL, MIN_COLS, MIN_ROWS};
    use anyhow::{bail, Context, Result};
    use crossterm::{
        cursor::{Hide, Show},
        execute,
        terminal::{
            self, disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
        },
    };
    use portable_pty::PtySize;
    use ratatui::{
        backend::CrosstermBackend, layout::Rect, text::Line, widgets::Paragraph, Terminal,
    };
    use std::io::Write;
    use std::process::ExitStatus;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    };
    use std::time::{Duration, Instant};

    struct TerminalGuard {
        restored: AtomicBool,
        #[cfg(unix)]
        tty: std::fs::File,
        #[cfg(unix)]
        mode: libc::termios,
    }

    impl TerminalGuard {
        fn enter() -> Result<Self> {
            #[cfg(unix)]
            let (tty, mode) = terminal_mode_snapshot()?;
            enable_raw_mode().context("failed to set raw terminal mode")?;
            let mut stdout = std::io::stdout();
            if let Err(error) = execute!(stdout, EnterAlternateScreen, Hide) {
                let _ = disable_raw_mode();
                #[cfg(unix)]
                restore_terminal_mode(&tty, &mode);
                return Err(error).context("failed to prepare the terminal for --tui");
            }
            if let Err(error) = stdout
                .write_all(super::MOUSE_CAPTURE_ENABLE)
                .and_then(|_| stdout.flush())
            {
                let _ = stdout.write_all(super::TERMINAL_MODE_RESET);
                let _ = execute!(stdout, LeaveAlternateScreen, Show);
                let _ = disable_raw_mode();
                #[cfg(unix)]
                restore_terminal_mode(&tty, &mode);
                return Err(error).context("failed to enable mouse input for --tui");
            }
            Ok(Self {
                restored: AtomicBool::new(false),
                #[cfg(unix)]
                tty,
                #[cfg(unix)]
                mode,
            })
        }

        fn restore(&self) {
            if self.restored.swap(true, Ordering::SeqCst) {
                return;
            }
            let mut stdout = std::io::stdout();
            let _ = stdout.write_all(super::TERMINAL_MODE_RESET);
            let _ = execute!(stdout, LeaveAlternateScreen, Show);
            let _ = disable_raw_mode();
            #[cfg(unix)]
            restore_terminal_mode(&self.tty, &self.mode);
            let _ = stdout.flush();
        }
    }

    #[cfg(unix)]
    fn terminal_mode_snapshot() -> Result<(std::fs::File, libc::termios)> {
        use std::os::fd::AsRawFd;

        let tty = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/tty")
            .context("failed to open the controlling terminal")?;
        let mut mode = unsafe { std::mem::zeroed() };
        if unsafe { libc::tcgetattr(tty.as_raw_fd(), &mut mode) } != 0 {
            return Err(std::io::Error::last_os_error()).context("failed to read terminal mode");
        }
        Ok((tty, mode))
    }

    #[cfg(unix)]
    fn restore_terminal_mode(tty: &std::fs::File, mode: &libc::termios) {
        use std::os::fd::AsRawFd;

        unsafe {
            libc::tcsetattr(tty.as_raw_fd(), libc::TCSANOW, mode);
        }
    }

    impl Drop for TerminalGuard {
        fn drop(&mut self) {
            self.restore();
        }
    }

    fn layout(rows: u16, cols: u16, info: &TuiStatusInfo) -> Result<(u16, u16)> {
        if cols < MIN_COLS {
            bail!("--tui needs at least {MIN_COLS} columns (current: {cols})");
        }
        let reserved_rows = super::content_line_count(cols, info) + 1;
        if rows < MIN_ROWS || rows <= reserved_rows + 2 {
            bail!("--tui needs a taller terminal to fit the status bar (needs at least {} rows, current: {rows})", reserved_rows + 3);
        }
        Ok((rows - reserved_rows, reserved_rows))
    }

    fn pty_size(rows: u16, cols: u16) -> PtySize {
        PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        }
    }

    fn draw(
        terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
        state: &TuiState,
        info: &TuiStatusInfo,
    ) -> Result<()> {
        let (status_text, status_rows) = super::render_frame(state.cols, info);
        terminal
            .draw(|frame| {
                let child = Rect::new(0, 0, state.cols, state.child_rows);
                state.screen.render(child, frame.buffer_mut());
                frame.render_widget(
                    Paragraph::new(status_text.lines().map(Line::from).collect::<Vec<_>>()),
                    Rect::new(0, state.child_rows, state.cols, status_rows),
                );
                if state.screen.cursor_visible() {
                    let (row, col) = state.screen.cursor_position();
                    if row < state.child_rows && col < state.cols {
                        frame.set_cursor_position((col, row));
                    }
                }
            })
            .context("failed to draw --tui")?;
        Ok(())
    }

    #[cfg(unix)]
    fn spawn_input_forwarder(
        writer: Arc<Mutex<Box<dyn Write + Send>>>,
        stop: Arc<AtomicBool>,
        mouse: mpsc::Sender<super::MouseAction>,
        child_captures_mouse: Arc<AtomicBool>,
        child_uses_alternate_scroll: Arc<AtomicBool>,
    ) -> std::thread::JoinHandle<()> {
        std::thread::spawn(move || {
            let mut buffer = [0u8; 4096];
            let mut pending = Vec::new();
            while !stop.load(Ordering::SeqCst) {
                let mut fds = libc::pollfd {
                    fd: 0,
                    events: libc::POLLIN,
                    revents: 0,
                };
                if unsafe { libc::poll(&mut fds, 1, 50) } <= 0 || fds.revents & libc::POLLIN == 0 {
                    continue;
                }
                let count = unsafe { libc::read(0, buffer.as_mut_ptr().cast(), buffer.len()) };
                if count <= 0 {
                    break;
                }
                let mut actions = Vec::new();
                let routing = if child_captures_mouse.load(Ordering::Relaxed) {
                    super::WheelRouting::Child
                } else if child_uses_alternate_scroll.load(Ordering::Relaxed) {
                    super::WheelRouting::AlternateScroll
                } else {
                    super::WheelRouting::Wrapper
                };
                let bytes = translate_mouse_input(
                    &mut pending,
                    &buffer[..count as usize],
                    &mut actions,
                    routing,
                );
                for action in actions {
                    let _ = mouse.send(action);
                }
                if writer
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .write_all(&bytes)
                    .is_err()
                {
                    break;
                }
            }
        })
    }

    /// Splits SGR mouse reports out of terminal input. With `Child` routing
    /// the child asked for mouse input and gets everything verbatim.
    /// Otherwise mouse reporting is on only for the wrapper: the vertical
    /// wheel scrolls (or, under `AlternateScroll`, becomes arrow keys), a
    /// left-button drag selects, and every other report is dropped.
    #[cfg(unix)]
    pub(super) fn translate_mouse_input(
        pending: &mut Vec<u8>,
        input: &[u8],
        actions: &mut Vec<super::MouseAction>,
        routing: super::WheelRouting,
    ) -> Vec<u8> {
        use super::{MouseAction, WheelRouting};
        pending.extend_from_slice(input);
        if matches!(routing, WheelRouting::Child) {
            return std::mem::take(pending);
        }
        let mut forwarded = Vec::with_capacity(pending.len());
        let mut typed = false;
        let mut index = 0;
        while index < pending.len() {
            let remaining = &pending[index..];
            if b"\x1b[<".starts_with(remaining) {
                break;
            }
            if !remaining.starts_with(b"\x1b[<") {
                forwarded.push(pending[index]);
                typed = true;
                index += 1;
                continue;
            }
            let Some(end) = remaining[3..]
                .iter()
                .position(|byte| matches!(byte, b'M' | b'm'))
            else {
                break;
            };
            let end = end + 4;
            let release = remaining[end - 1] == b'm';
            let fields: Vec<u16> = std::str::from_utf8(&remaining[3..end - 1])
                .ok()
                .map(|event| {
                    event
                        .split(';')
                        .filter_map(|field| field.parse().ok())
                        .collect()
                })
                .unwrap_or_default();
            if let [button, col, row] = fields[..] {
                let (row, col) = (row.saturating_sub(1), col.saturating_sub(1));
                // Low two bits: button; 4/8/16: modifiers; 32: motion; 64: wheel.
                let motion = button & 32 != 0;
                match (button & !0b1_1100, routing) {
                    (64, WheelRouting::Wrapper) => actions.push(MouseAction::Scroll(3)),
                    (65, WheelRouting::Wrapper) => actions.push(MouseAction::Scroll(-3)),
                    (64, WheelRouting::AlternateScroll) => {
                        forwarded.extend_from_slice(b"\x1b[A\x1b[A\x1b[A")
                    }
                    (65, WheelRouting::AlternateScroll) => {
                        forwarded.extend_from_slice(b"\x1b[B\x1b[B\x1b[B")
                    }
                    (0, _) if release => actions.push(MouseAction::Release { row, col }),
                    (0, _) => actions.push(MouseAction::Press { row, col }),
                    (32, _) if motion => actions.push(MouseAction::Drag { row, col }),
                    _ => {}
                }
            }
            index += end;
        }
        pending.drain(..index);
        if typed {
            actions.insert(0, MouseAction::ClearSelection);
        }
        forwarded
    }

    #[cfg(not(unix))]
    fn spawn_input_forwarder(
        writer: Arc<Mutex<Box<dyn Write + Send>>>,
        stop: Arc<AtomicBool>,
        _mouse: mpsc::Sender<super::MouseAction>,
        _child_captures_mouse: Arc<AtomicBool>,
        _child_uses_alternate_scroll: Arc<AtomicBool>,
    ) -> std::thread::JoinHandle<()> {
        std::thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                if !crossterm::event::poll(Duration::from_millis(50)).unwrap_or(false) {
                    continue;
                }
                let Ok(crossterm::event::Event::Key(key)) = crossterm::event::read() else {
                    continue;
                };
                let bytes = match key.code {
                    crossterm::event::KeyCode::Char(character) => {
                        character.to_string().into_bytes()
                    }
                    crossterm::event::KeyCode::Enter => b"\r".to_vec(),
                    crossterm::event::KeyCode::Esc => b"\x1b".to_vec(),
                    crossterm::event::KeyCode::Backspace => b"\x7f".to_vec(),
                    crossterm::event::KeyCode::Left => b"\x1b[D".to_vec(),
                    crossterm::event::KeyCode::Right => b"\x1b[C".to_vec(),
                    crossterm::event::KeyCode::Up => b"\x1b[A".to_vec(),
                    crossterm::event::KeyCode::Down => b"\x1b[B".to_vec(),
                    _ => continue,
                };
                if writer
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .write_all(&bytes)
                    .is_err()
                {
                    break;
                }
            }
        })
    }

    pub fn run_command_in_tui(
        command: super::TuiCommand,
        info: TuiStatusInfo,
    ) -> Result<ExitStatus> {
        if !super::tui_supported() {
            bail!(super::tui_unsupported_reason());
        }
        let (cols, rows) = terminal::size().context("failed to read terminal size")?;
        let (child_rows, _) = layout(rows, cols, &info)?;
        let guard = TerminalGuard::enter()?;
        let mut terminal = Terminal::new(CrosstermBackend::new(std::io::stdout()))
            .context("failed to initialize --tui")?;
        terminal.clear().context("failed to clear --tui")?;
        let (events, receiver) = mpsc::channel();
        let (mouse, mouse_receiver) = mpsc::channel();
        let mut session = match pty::PtySession::spawn(command, pty_size(child_rows, cols), events)
        {
            Ok(session) => session,
            Err(error) => {
                guard.restore();
                return Err(error);
            }
        };
        // The child exists now: an `agent run` counts as launched for
        // telemetry only from this point.
        crate::telemetry::mark_agent_run_launched();
        let stop = Arc::new(AtomicBool::new(false));
        let child_captures_mouse = Arc::new(AtomicBool::new(false));
        let child_uses_alternate_scroll = Arc::new(AtomicBool::new(false));
        let input = spawn_input_forwarder(
            session.input_writer(),
            stop.clone(),
            mouse,
            child_captures_mouse.clone(),
            child_uses_alternate_scroll.clone(),
        );
        let start = Instant::now();
        let mut state = TuiState::new(child_rows, cols, start);
        let mut output_closed = false;
        let mut exit = None;
        let mut exited_at = None;
        let result = loop {
            while let Ok(event) = receiver.try_recv() {
                match event {
                    pty::TuiEvent::Output(bytes) => state.output(&bytes),
                    pty::TuiEvent::OutputClosed => output_closed = true,
                }
            }
            while let Ok(action) = mouse_receiver.try_recv() {
                state.mouse(action);
            }
            child_captures_mouse.store(state.child_captures_mouse(), Ordering::Relaxed);
            child_uses_alternate_scroll
                .store(state.child_uses_alternate_scroll(), Ordering::Relaxed);
            if let Some(modes) = state.take_input_mode_update() {
                let mut stdout = std::io::stdout();
                let modes = super::physical_input_modes(&modes, state.child_captures_mouse());
                let _ = stdout.write_all(&modes);
                let _ = stdout.flush();
            }
            let now = Instant::now();
            if exit.is_none() {
                exit = session.try_wait()?;
                if exit.is_some() {
                    exited_at = Some(now);
                }
            }
            let (new_cols, new_rows) = terminal::size().context("failed to read terminal size")?;
            if (new_rows, new_cols)
                != (
                    state.child_rows + super::content_line_count(state.cols, &info) + 1,
                    state.cols,
                )
            {
                if let Ok((new_child_rows, _)) = layout(new_rows, new_cols, &info) {
                    session.resize(pty_size(new_child_rows, new_cols))?;
                    terminal
                        .resize(Rect::new(0, 0, new_cols, new_rows))
                        .context("failed to resize --tui")?;
                    state.resize(new_child_rows, new_cols);
                }
            }
            if state.should_draw(now) {
                draw(&mut terminal, &state, &info)?;
                state.did_draw(now);
            }
            if exit.is_some() && output_closed {
                break Ok(exit.unwrap());
            }
            if exited_at
                .is_some_and(|exited_at| now.duration_since(exited_at) > Duration::from_millis(150))
            {
                break Ok(exit.unwrap());
            }
            let wait = if state.dirty {
                state
                    .next_draw
                    .saturating_duration_since(Instant::now())
                    .min(FRAME_INTERVAL)
            } else {
                Duration::from_millis(20)
            };
            if let Ok(event) = receiver.recv_timeout(wait) {
                match event {
                    pty::TuiEvent::Output(bytes) => state.output(&bytes),
                    pty::TuiEvent::OutputClosed => output_closed = true,
                }
            }
        };
        stop.store(true, Ordering::SeqCst);
        let _ = input.join();
        if result.is_err() {
            let _ = session.interrupt();
        }
        drop(session);
        drop(terminal);
        guard.restore();
        result.map(portable_exit_status)
    }

    #[cfg(unix)]
    fn portable_exit_status(status: portable_pty::ExitStatus) -> ExitStatus {
        use std::os::unix::process::ExitStatusExt;
        ExitStatus::from_raw((status.exit_code() as i32) << 8)
    }
    #[cfg(windows)]
    fn portable_exit_status(status: portable_pty::ExitStatus) -> ExitStatus {
        use std::os::windows::process::ExitStatusExt;
        ExitStatus::from_raw(status.exit_code())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::ffi::OsStr;
    use std::path::PathBuf;

    fn sample_info() -> TuiStatusInfo {
        TuiStatusInfo {
            profile: "coding".to_owned(),
            global_profile: false,
            mode: TuiMode::Remote,
            docker_sandbox: false,
            session_id: Some("ags_wAiPhZv2K9mX".to_owned()),
            egress_host_count: 3,
            filesystem_read_denial_count: 0,
            filesystem_write_denial_count: 0,
            shared_secret_count: 2,
            personal_credential_count: 1,
            mcp_server_count: 1,
            mcp_allowed_tool_count: Some(4),
            project_environment: None,
        }
    }

    #[test]
    fn status_bar_shows_profile_mode_session_and_counts() {
        let (frame, _rows) = render_frame(80, &sample_info());
        assert!(frame.contains("coding"));
        assert!(frame.contains("remote"));
        assert!(frame.contains("ags_wAiPhZv2K9mX"));
        assert!(frame.contains("Egress: 3 hosts"));
        assert!(frame.contains("Bindings: 2 secrets, 1 personal"));
        assert!(frame.contains("MCP: 4 allowed"));
        assert!(frame.find("Egress:") < frame.find("Bindings:"));
        assert!(frame.find("Bindings:") < frame.find("MCP:"));
    }

    #[test]
    fn status_bar_has_no_leading_indicator_glyph() {
        let (frame, _rows) = render_frame(80, &sample_info());
        for glyph in ["●", "○", "◐", "✖"] {
            assert!(
                !frame.contains(glyph),
                "unexpected glyph {glyph:?} in {frame:?}"
            );
        }
    }

    #[test]
    fn local_mode_omits_a_session_id_when_absent() {
        let mut info = sample_info();
        info.mode = TuiMode::Local;
        info.session_id = None;
        let (frame, _rows) = render_frame(80, &info);
        assert!(frame.contains("local"));
        assert!(!frame.contains("ags_wAiPhZv2K9mX"));
    }

    #[test]
    fn status_bar_marks_docker_sandbox_runs() {
        let mut info = sample_info();
        info.mode = TuiMode::Local;
        let (frame, _rows) = render_frame(80, &info);
        assert!(!frame.contains("docker"));
        info.docker_sandbox = true;
        let (frame, _rows) = render_frame(80, &info);
        assert!(frame.contains("local · docker"));
    }

    #[test]
    fn status_bar_marks_global_profiles() {
        let mut info = sample_info();
        info.global_profile = true;
        let (frame, _rows) = render_frame(80, &info);
        assert!(frame.contains("global"));
    }

    #[test]
    fn status_bar_shows_filesystem_rule_counts_when_configured() {
        let mut info = sample_info();
        info.filesystem_read_denial_count = 2;
        info.filesystem_write_denial_count = 1;
        let (frame, _rows) = render_frame(120, &info);
        assert!(frame.contains("Filesystem: 2 read, 1 write denied"));
        assert!(frame.find("Filesystem:") < frame.find("Egress:"));
        assert!(frame.find("Egress:") < frame.find("Bindings:"));
    }

    #[test]
    fn status_bar_omits_unconfigured_filesystem_rule_types() {
        let mut info = sample_info();
        info.filesystem_read_denial_count = 2;
        let (frame, _rows) = render_frame(120, &info);
        assert!(frame.contains("Filesystem: 2 read denied"));
        assert!(!frame.contains("write"));
    }

    #[test]
    fn status_bar_omits_empty_bindings_and_absent_mcp() {
        let mut info = sample_info();
        info.shared_secret_count = 0;
        info.personal_credential_count = 0;
        info.mcp_server_count = 0;
        let (frame, _rows) = render_frame(120, &info);
        assert!(!frame.contains("Bindings:"));
        assert!(!frame.contains("MCP:"));
    }

    #[test]
    fn status_bar_omits_unconfigured_binding_types() {
        let mut info = sample_info();
        info.personal_credential_count = 0;
        let (frame, _rows) = render_frame(120, &info);
        assert!(frame.contains("Bindings: 2 secrets"));
        assert!(!frame.contains("personal"));
    }

    #[test]
    fn unrestricted_mcp_tools_render_without_a_misleading_count() {
        let mut info = sample_info();
        info.mcp_allowed_tool_count = None;
        let (frame, _rows) = render_frame(80, &info);
        assert!(frame.contains("MCP: unrestricted"));
    }

    #[test]
    fn content_that_does_not_fit_wraps_onto_more_lines_instead_of_truncating() {
        let info = sample_info();
        let cols = 50;
        let (frame, rows) = render_frame(cols, &info);
        // All the identity and metrics chips must still be present somewhere
        // in the frame; a terminal too narrow for one line should make the
        // bar taller, not cut or truncate content out of it.
        assert!(frame.contains("coding"));
        assert!(frame.contains("remote"));
        assert!(frame.contains("ags_wAiPhZv2K9mX"));
        assert!(frame.contains("Egress: 3 hosts"));
        assert!(frame.contains("Bindings: 2 secrets, 1 personal"));
        assert!(frame.contains("MCP: 4 allowed"));
        assert!(
            rows > 1 + 1,
            "content wider than the terminal should need more than one content line"
        );
        for line in frame.lines() {
            assert!(
                line.chars().count() <= cols as usize,
                "line exceeded width: {line:?}"
            );
        }
    }

    #[test]
    fn status_bar_content_has_horizontal_padding() {
        let (frame, _rows) = render_frame(80, &sample_info());
        for line in frame.lines().skip(1) {
            assert!(line.starts_with(' '));
            assert!(line.chars().count() < 80);
        }
    }

    #[test]
    fn a_single_chip_wider_than_the_terminal_is_truncated_with_an_ellipsis() {
        let mut info = sample_info();
        info.profile =
            "a-very-long-profile-name-that-cannot-possibly-fit-on-one-narrow-line".to_owned();
        let (frame, _rows) = render_frame(24, &info);
        assert!(frame.contains('…'));
        for line in frame.lines() {
            assert!(line.chars().count() <= 24, "line exceeded width: {line:?}");
        }
    }

    #[test]
    fn bar_height_never_exceeds_the_configured_maximum() {
        let mut info = sample_info();
        info.profile = "a-very-long-profile-name-that-does-not-fit-in-a-narrow-terminal".to_owned();
        info.project_environment = Some("some-org/some-very-long-environment-name".to_owned());
        let (_frame, rows) = render_frame(20, &info);
        // 1 separator line + at most MAX_CONTENT_LINES content lines.
        assert!(rows <= 1 + MAX_CONTENT_LINES);
    }

    #[test]
    fn bar_height_is_stable_for_the_same_terminal_width() {
        let info = sample_info();
        let (_first, first_rows) = render_frame(40, &info);
        let (_second, second_rows) = render_frame(40, &info);
        assert_eq!(first_rows, second_rows);
    }

    #[test]
    fn mcp_tool_count_requires_every_server_to_declare_an_allowlist() {
        use crate::models::agent::AgentMcpServer;

        let mut servers = HashMap::new();
        servers.insert(
            "github".to_owned(),
            AgentMcpServer {
                url: "https://example.invalid/mcp".to_owned(),
                binding: None,
                header: None,
                value_template: None,
                allow_tools: vec!["search".to_owned(), "read".to_owned()],
                deny_tools: Vec::new(),
            },
        );
        assert_eq!(
            TuiStatusInfo::mcp_allowed_tool_count_from(&servers),
            Some(2)
        );

        servers.insert(
            "unrestricted".to_owned(),
            AgentMcpServer {
                url: "https://example.invalid/other".to_owned(),
                binding: None,
                header: None,
                value_template: None,
                allow_tools: Vec::new(),
                deny_tools: Vec::new(),
            },
        );
        assert_eq!(TuiStatusInfo::mcp_allowed_tool_count_from(&servers), None);

        assert_eq!(
            TuiStatusInfo::mcp_allowed_tool_count_from(&HashMap::new()),
            None
        );
    }

    /// The status bar must never be able to leak a secret value even if one
    /// were mistakenly threaded into a field meant for a name or count. This
    /// pins the struct's shape so a future field addition has to be a count
    /// or identifier, not a value, to compile against this test's usage.
    #[test]
    fn status_info_carries_only_counts_and_identifiers_no_secret_values() {
        let info = TuiStatusInfo {
            profile: "coding".to_owned(),
            global_profile: false,
            mode: TuiMode::Local,
            docker_sandbox: false,
            session_id: None,
            egress_host_count: 0,
            filesystem_read_denial_count: 0,
            filesystem_write_denial_count: 0,
            shared_secret_count: 0,
            personal_credential_count: 0,
            mcp_server_count: 0,
            mcp_allowed_tool_count: None,
            project_environment: Some("acme/production".to_owned()),
        };
        let (rendered, _rows) = render_frame(80, &info);
        // Only names/counts we deliberately put in should ever appear;
        // nothing resembling a secret value literal is constructed anywhere
        // in `render_frame`.
        assert!(!rendered.contains("Bearer "));
        assert!(!rendered.contains("sk_"));
    }

    #[test]
    fn rapid_output_coalesces_without_a_heartbeat() {
        let start = Instant::now();
        let mut state = TuiState::new(20, 80, start);
        state.did_draw(start);
        state.output(b"a");
        state.output(b"b");

        assert!(!state.should_draw(start + Duration::from_millis(15)));
        assert!(state.should_draw(start + Duration::from_millis(16)));
        state.did_draw(start + Duration::from_millis(16));
        assert!(!state.should_draw(start + Duration::from_secs(10)));
    }

    #[test]
    fn terminal_cleanup_disables_synchronized_output() {
        assert!(TERMINAL_MODE_RESET
            .windows(b"\x1b[?2026l".len())
            .any(|mode| mode == b"\x1b[?2026l"));
    }

    #[cfg(unix)]
    fn translate(input: &[u8], routing: WheelRouting) -> (Vec<u8>, Vec<MouseAction>) {
        let mut pending = Vec::new();
        let mut actions = Vec::new();
        let forwarded =
            super::imp::translate_mouse_input(&mut pending, input, &mut actions, routing);
        (forwarded, actions)
    }

    #[cfg(unix)]
    #[test]
    fn mouse_wheel_scrolls_wrapper_history_without_touching_other_input() {
        let mut pending = Vec::new();
        let mut actions = Vec::new();
        assert_eq!(
            super::imp::translate_mouse_input(
                &mut pending,
                b"x\x1b[<64;10;5M",
                &mut actions,
                WheelRouting::Wrapper
            ),
            b"x"
        );
        assert_eq!(
            actions,
            [MouseAction::ClearSelection, MouseAction::Scroll(3)]
        );
        actions.clear();
        assert_eq!(
            super::imp::translate_mouse_input(
                &mut pending,
                b"\x1b[<65;",
                &mut actions,
                WheelRouting::Wrapper,
            ),
            b""
        );
        assert!(actions.is_empty());
        assert_eq!(
            super::imp::translate_mouse_input(
                &mut pending,
                b"10;5M",
                &mut actions,
                WheelRouting::Wrapper,
            ),
            b""
        );
        assert_eq!(actions, [MouseAction::Scroll(-3)]);
        assert_eq!(
            translate(b"\x1b[A", WheelRouting::Wrapper),
            (b"\x1b[A".to_vec(), vec![MouseAction::ClearSelection])
        );
        assert_eq!(
            translate(b"\x1b[<64;10;5M", WheelRouting::Child).0,
            b"\x1b[<64;10;5M"
        );
        assert_eq!(
            translate(b"\x1b[<64;10;5M", WheelRouting::AlternateScroll),
            (b"\x1b[A\x1b[A\x1b[A".to_vec(), Vec::new())
        );
    }

    #[cfg(unix)]
    #[test]
    fn wrapper_turns_left_drag_into_a_selection_instead_of_leaking_it() {
        // Press, drag, release (1-based col;row), then a right click and a
        // horizontal wheel: none of these reach a child that has not
        // enabled mouse reporting.
        let clicks = b"\x1b[<0;10;5M\x1b[<32;11;5M\x1b[<0;11;6m\x1b[<2;3;3M\x1b[<66;10;5M";
        for routing in [WheelRouting::Wrapper, WheelRouting::AlternateScroll] {
            assert_eq!(
                translate(clicks, routing),
                (
                    Vec::new(),
                    vec![
                        MouseAction::Press { row: 4, col: 9 },
                        MouseAction::Drag { row: 4, col: 10 },
                        MouseAction::Release { row: 5, col: 10 },
                    ]
                )
            );
        }
        assert_eq!(
            translate(clicks, WheelRouting::Child),
            (clicks.to_vec(), Vec::new())
        );
    }

    #[test]
    fn child_mouse_protocol_is_not_overridden_by_wrapper_capture() {
        let child_modes = b"\x1b[?1000h";
        let owned_by_child = physical_input_modes(child_modes, true);
        assert!(owned_by_child.starts_with(MOUSE_CAPTURE_DISABLE));
        assert!(owned_by_child.ends_with(child_modes));

        let owned_by_wrapper = physical_input_modes(child_modes, false);
        assert!(owned_by_wrapper.ends_with(MOUSE_CAPTURE_ENABLE));
    }

    #[test]
    fn tui_command_preserves_program_args_cwd_and_environment_policy() {
        let command = TuiCommand {
            program: "sandbox-exec".into(),
            args: vec!["-p".into(), "policy".into(), "claude".into()],
            cwd: PathBuf::from("/tmp/project"),
            env: vec![("ANTHROPIC_API_KEY".into(), "proxy-placeholder".into())],
            env_removals: vec!["CLAUDE_CODE_OAUTH_TOKEN".into()],
        };

        let builder = command.into_builder();
        assert_eq!(
            builder.get_argv(),
            &["sandbox-exec", "-p", "policy", "claude"]
        );
        assert_eq!(
            builder.get_cwd().map(OsString::as_os_str),
            Some(OsStr::new("/tmp/project"))
        );
        assert_eq!(
            builder.get_env("ANTHROPIC_API_KEY"),
            Some(OsStr::new("proxy-placeholder"))
        );
        assert_eq!(builder.get_env("CLAUDE_CODE_OAUTH_TOKEN"), None);
        assert_eq!(builder.get_env("TERM"), std::env::var_os("TERM").as_deref());
        assert_eq!(
            builder.get_env("COLORTERM"),
            std::env::var_os("COLORTERM").as_deref()
        );
    }
}
