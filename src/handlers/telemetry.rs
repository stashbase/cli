use std::{io::Write, path::Path};

use anyhow::{Context, Result};

use crate::{
    cmd::telemetry::TelemetrySubcommand,
    telemetry::{
        consent::{off_reason, Signals},
        state::{self, State},
    },
};

pub fn handle_telemetry_command(subcommand: TelemetrySubcommand) -> Result<()> {
    let path = state::state_path().context("could not determine the config directory")?;
    run(
        subcommand,
        &path,
        &Signals::from_process(),
        &mut std::io::stdout(),
    )
}

pub fn run(
    subcommand: TelemetrySubcommand,
    path: &Path,
    signals: &Signals,
    out: &mut impl Write,
) -> Result<()> {
    let mut state = state::load(path);
    match subcommand {
        TelemetrySubcommand::Enable => {
            state.enabled = Some(true);
            state.notice_shown = true;
            state::save(path, &state)?;
            writeln!(
                out,
                "Telemetry enabled. See docs/telemetry.md for exactly what is sent."
            )?;
        }
        TelemetrySubcommand::Disable => {
            state.enabled = Some(false);
            state::save(path, &state)?;
            writeln!(out, "Telemetry disabled.")?;
        }
        TelemetrySubcommand::Status => print_status(&state, signals, out)?,
    }
    Ok(())
}

/// `enabled` or `disabled (<reason>)`: the one-line state shown by
/// `telemetry status` and `config print`.
pub fn summary(signals: &Signals, state: &State) -> String {
    match off_reason(signals, state) {
        Some(reason) => format!("disabled ({reason})"),
        None => "enabled".to_owned(),
    }
}

/// The summary for this process: the stored choice plus the current
/// environment (CI, sandbox, `DO_NOT_TRACK`, ...).
pub fn current_summary() -> String {
    match state::state_path() {
        Some(path) => summary(&Signals::from_process(), &state::load(&path)),
        None => "unknown".to_owned(),
    }
}

fn print_status(state: &State, signals: &Signals, out: &mut impl Write) -> Result<()> {
    writeln!(out, "Telemetry: {}", summary(signals, state))?;
    if !state.notice_shown {
        writeln!(
            out,
            "Nothing is sent until the first-run notice has been shown in an interactive terminal."
        )?;
    }
    writeln!(out, "Schema and opt-out options: docs/telemetry.md")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use clap::Parser;
    use uuid::Uuid;

    use super::*;
    use crate::{
        cmd::root::Cli,
        telemetry::{event::TrackedCommand, state},
    };

    fn temp_path() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("stashbase-telemetry-cmd-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("telemetry.json")
    }

    fn run_to_string(
        sub: TelemetrySubcommand,
        path: &std::path::Path,
        signals: &Signals,
    ) -> String {
        let mut out = Vec::new();
        run(sub, path, signals, &mut out).unwrap();
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn disable_then_enable_updates_state_and_marks_notice_seen() {
        let path = temp_path();
        run_to_string(TelemetrySubcommand::Disable, &path, &Signals::default());
        assert_eq!(state::load(&path).enabled, Some(false));

        run_to_string(TelemetrySubcommand::Enable, &path, &Signals::default());
        let loaded = state::load(&path);
        assert_eq!(loaded.enabled, Some(true));
        assert!(
            loaded.notice_shown,
            "explicitly enabling counts as having seen the notice"
        );
    }

    #[test]
    fn status_reports_the_reason_when_off() {
        let path = temp_path();
        run_to_string(TelemetrySubcommand::Disable, &path, &Signals::default());
        let text = run_to_string(TelemetrySubcommand::Status, &path, &Signals::default());
        assert!(text.contains("disabled"), "{text}");
        assert!(text.contains("telemetry disable"), "{text}");

        let in_ci = Signals {
            ci: true,
            ..Signals::default()
        };
        let text = run_to_string(TelemetrySubcommand::Status, &path, &in_ci);
        assert!(text.contains("CI"), "{text}");
    }

    #[test]
    fn status_says_nothing_is_sent_before_the_notice() {
        let path = temp_path();
        let text = run_to_string(TelemetrySubcommand::Status, &path, &Signals::default());
        assert!(text.contains("first-run notice"), "{text}");
    }

    #[test]
    fn there_is_no_reset_subcommand() {
        assert!(Cli::try_parse_from(["stashbase", "config", "telemetry", "reset"]).is_err());
    }

    #[test]
    fn the_telemetry_command_itself_is_never_tracked() {
        for sub in ["enable", "disable", "status"] {
            let cli = Cli::try_parse_from(["stashbase", "config", "telemetry", sub]).unwrap();
            assert!(
                TrackedCommand::from_entity(&cli.entity_type).is_none(),
                "{sub}"
            );
            assert!(!cli.entity_type.requires_api_key(), "{sub}");
        }
    }

    #[test]
    fn the_summary_says_enabled_or_why_it_is_off() {
        assert_eq!(
            summary(&Signals::default(), &state::State::default()),
            "enabled"
        );

        let ci = Signals {
            ci: true,
            ..Signals::default()
        };
        assert_eq!(
            summary(&ci, &state::State::default()),
            "disabled (running in CI)"
        );

        let opted_out = state::State {
            enabled: Some(false),
            ..state::State::default()
        };
        let text = summary(&Signals::default(), &opted_out);
        assert!(text.starts_with("disabled (opted out with"), "{text}");
        assert!(
            text.contains("stashbase config telemetry disable"),
            "{text}"
        );
    }
}
