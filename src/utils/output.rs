use crate::{
    cmd::{config::OutputFormat, root::ColorChoice},
    COLOR_CHOICE,
};
use anyhow::Result;
use colored_json::to_colored_json_auto;
use owo_colors::OwoColorize;
use std::io::IsTerminal;

pub fn get_output_format(
    raw_output: bool,
    default_output_format: Option<OutputFormat>,
    cmd_format: Option<OutputFormat>,
) -> OutputFormat {
    match raw_output {
        true => OutputFormat::Json,
        false => cmd_format.unwrap_or(default_output_format.unwrap_or_default()),
    }
}

pub fn get_is_json_output_format(
    raw_output: bool,
    default_output_format: Option<OutputFormat>,
) -> bool {
    match raw_output {
        true => true,
        false => match default_output_format == Some(OutputFormat::Json) {
            true => true,
            false => false,
        },
    }
}
pub fn write_indented(f: &mut std::fmt::Formatter<'_>, indent: usize, s: &str) -> std::fmt::Result {
    let indent_str = " ".repeat(indent);
    for line in s.lines() {
        writeln!(f, "{}{}", indent_str, line)?;
    }

    Ok(())
}

pub fn get_colored_json<T: serde::Serialize>(data: &T) -> Result<String> {
    let value = serde_json::to_value(data)?;
    let json_str = to_colored_json_auto(&value)?;

    Ok(json_str)
}

/// Converts data to a JSON string with optional color formatting
///
/// ### Arguments
/// * `data` - The data to serialize to JSON
/// * `stdout` - Whether to check stdout (true) or stderr (false) for color support
///
/// ### Returns
/// * `Result<String>` - The formatted JSON string, with colors if enabled
pub fn get_formatted_json_string<T: serde::Serialize>(
    data: &T,
    stdout: bool,
) -> Result<String, serde_json::Error> {
    let value = serde_json::to_value(data)?;

    let json_str = if is_color_enabled(stdout) {
        to_colored_json_auto(&value)?
    } else {
        serde_json::to_string_pretty(&value)?
    };

    Ok(json_str)
}

/// Check if stdout is a terminal
pub fn is_terminal() -> bool {
    std::io::stdout().is_terminal()
}

/// Check if stderr is a terminal
pub fn is_terminal_stderr() -> bool {
    std::io::stderr().is_terminal()
}

/// Check if colors should be enabled
/// - `stdout`: true for stdout detection, false for stderr detection
pub fn is_color_enabled(stdout: bool) -> bool {
    let color_choice = COLOR_CHOICE.get().unwrap_or(&ColorChoice::Auto);

    match color_choice {
        ColorChoice::Auto => {
            // Force enable color
            if std::env::var("FORCE_COLOR").is_ok() || std::env::var("CLICOLOR_FORCE").is_ok() {
                return true;
            }

            // Disable color conditions
            if std::env::var("NO_COLOR").is_ok()
                || std::env::var("NOCOLOR").is_ok()
                || std::env::var("TERM").map_or(false, |term| term == "dumb")
            {
                return false;
            }

            // Check terminal capability
            if stdout {
                is_terminal()
            } else {
                is_terminal_stderr()
            }
        }
        ColorChoice::Always => true,
        ColorChoice::Never => false,
    }
}

/// Trait to colorize text if stdout/stderr is a terminal and color is enabled
pub trait ColorizeIfColoredOutput {
    fn green_if_tty(self) -> String;
    fn red_if_tty(self) -> String;
    fn yellow_if_tty(self) -> String;
    fn blue_if_tty(self) -> String;
    fn bright_black_if_tty(self) -> String;
    fn bright_blue_if_tty(self) -> String;

    // stderr
    fn green_if_tty_stderr(self) -> String;
    fn red_if_tty_stderr(self) -> String;
    fn yellow_if_tty_stderr(self) -> String;
    fn blue_if_tty_stderr(self) -> String;
    fn bright_black_if_tty_stderr(self) -> String;
    fn bright_blue_if_tty_stderr(self) -> String;
}

/// Implementation of the ColorizeIfColoredOutput trait
impl<T: OwoColorize + std::fmt::Display> ColorizeIfColoredOutput for T {
    fn green_if_tty(self) -> String {
        if is_color_enabled(true) {
            format!("{}", self.green())
        } else {
            format!("{}", self)
        }
    }

    fn red_if_tty(self) -> String {
        if is_color_enabled(true) {
            format!("{}", self.red())
        } else {
            format!("{}", self)
        }
    }

    fn yellow_if_tty(self) -> String {
        if is_color_enabled(true) {
            format!("{}", self.yellow())
        } else {
            format!("{}", self)
        }
    }

    fn blue_if_tty(self) -> String {
        if is_color_enabled(true) {
            format!("{}", self.blue())
        } else {
            format!("{}", self)
        }
    }

    // stderr
    fn green_if_tty_stderr(self) -> String {
        if is_color_enabled(false) {
            format!("{}", self.green())
        } else {
            format!("{}", self)
        }
    }

    fn red_if_tty_stderr(self) -> String {
        if is_color_enabled(false) {
            format!("{}", self.red())
        } else {
            format!("{}", self)
        }
    }

    fn yellow_if_tty_stderr(self) -> String {
        if is_color_enabled(false) {
            format!("{}", self.yellow())
        } else {
            format!("{}", self)
        }
    }

    fn blue_if_tty_stderr(self) -> String {
        if is_color_enabled(false) {
            format!("{}", self.blue())
        } else {
            format!("{}", self)
        }
    }

    fn bright_black_if_tty_stderr(self) -> String {
        if is_color_enabled(false) {
            format!("{}", self.bright_black())
        } else {
            format!("{}", self)
        }
    }

    fn bright_black_if_tty(self) -> String {
        if is_color_enabled(true) {
            format!("{}", self.bright_black())
        } else {
            format!("{}", self)
        }
    }

    fn bright_blue_if_tty(self) -> String {
        if is_color_enabled(true) {
            format!("{}", self.bright_blue())
        } else {
            format!("{}", self)
        }
    }

    fn bright_blue_if_tty_stderr(self) -> String {
        if is_color_enabled(false) {
            format!("{}", self.bright_blue())
        } else {
            format!("{}", self)
        }
    }
}
