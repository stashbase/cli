use crate::cmd::config::OutputFormat;
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

pub fn is_terminal() -> bool {
    std::io::stdout().is_terminal()
}

pub fn is_terminal_stderr() -> bool {
    std::io::stderr().is_terminal()
}

pub trait ColorizeIfTerminal {
    fn green_if_tty(self) -> String;
    fn red_if_tty(self) -> String;
    fn yellow_if_tty(self) -> String;
    fn blue_if_tty(self) -> String;

    // stderr
    fn green_if_tty_stderr(self) -> String;
    fn red_if_tty_stderr(self) -> String;
    fn yellow_if_tty_stderr(self) -> String;
    fn blue_if_tty_stderr(self) -> String;
}

impl<T: OwoColorize + std::fmt::Display> ColorizeIfTerminal for T {
    fn green_if_tty(self) -> String {
        if is_terminal() {
            format!("{}", self.green())
        } else {
            format!("{}", self)
        }
    }

    fn red_if_tty(self) -> String {
        if is_terminal() {
            format!("{}", self.red())
        } else {
            format!("{}", self)
        }
    }

    fn yellow_if_tty(self) -> String {
        if is_terminal() {
            format!("{}", self.yellow())
        } else {
            format!("{}", self)
        }
    }

    fn blue_if_tty(self) -> String {
        if is_terminal() {
            format!("{}", self.blue())
        } else {
            format!("{}", self)
        }
    }

    // stderr
    fn green_if_tty_stderr(self) -> String {
        if is_terminal_stderr() {
            format!("{}", self.green())
        } else {
            format!("{}", self)
        }
    }

    fn red_if_tty_stderr(self) -> String {
        if is_terminal_stderr() {
            format!("{}", self.red())
        } else {
            format!("{}", self)
        }
    }

    fn yellow_if_tty_stderr(self) -> String {
        if is_terminal_stderr() {
            format!("{}", self.yellow())
        } else {
            format!("{}", self)
        }
    }

    fn blue_if_tty_stderr(self) -> String {
        if is_terminal_stderr() {
            format!("{}", self.blue())
        } else {
            format!("{}", self)
        }
    }
}
