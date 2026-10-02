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
            writeln!(out, "Telemetry enabled. See docs/telemetry.md for exactly what is sent.")?;
        }
        TelemetrySubcommand::Disable => {
            state.enabled = Some(false);
            state::save(path, &state)?;
            writeln!(out, "Telemetry disabled.")?;
        }
        TelemetrySubcommand::Reset => {
            state.install_id = None;
            state::save(path, &state)?;
            writeln!(out, "Install ID reset.")?;
        }
        TelemetrySubcommand::Status => print_status(&state, signals, out)?,
    }
    Ok(())
}

fn print_status(state: &State, signals: &Signals, out: &mut impl Write) -> Result<()> {
    match off_reason(signals, state) {
        Some(reason) => writeln!(out, "Telemetry: disabled ({reason})")?,
        None => writeln!(out, "Telemetry: enabled")?,
    }
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

    fn run_to_string(sub: TelemetrySubcommand, path: &std::path::Path, signals: &Signals) -> String {
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
        assert!(loaded.notice_shown, "explicitly enabling counts as having seen the notice");
    }

    #[test]
    fn status_reports_the_reason_when_off() {
        let path = temp_path();
        run_to_string(TelemetrySubcommand::Disable, &path, &Signals::default());
        let text = run_to_string(TelemetrySubcommand::Status, &path, &Signals::default());
        assert!(text.contains("disabled"), "{text}");
        assert!(text.contains("telemetry disable"), "{text}");

        let in_ci = Signals { ci: true, ..Signals::default() };
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
    fn reset_clears_the_install_id() {
        let path = temp_path();
        let mut s = state::State::default();
        s.ensure_install_id();
        state::save(&path, &s).unwrap();

        run_to_string(TelemetrySubcommand::Reset, &path, &Signals::default());
        assert_eq!(state::load(&path).install_id, None);
    }

    #[test]
    fn the_telemetry_command_itself_is_never_tracked() {
        for sub in ["enable", "disable", "status", "reset"] {
            let cli = Cli::try_parse_from(["stashbase", "telemetry", sub]).unwrap();
            assert!(TrackedCommand::from_entity(&cli.entity_type).is_none(), "{sub}");
            assert!(!cli.entity_type.requires_api_key(), "{sub}");
        }
    }
}
