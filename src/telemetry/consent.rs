use std::io::{IsTerminal, Write};

use super::state::State;

const CI_VARS: &[&str] = &[
    "CI",
    "GITHUB_ACTIONS",
    "GITLAB_CI",
    "BUILDKITE",
    "CIRCLECI",
    "JENKINS_URL",
    "TF_BUILD",
];

#[derive(Debug, Default, Clone)]
pub struct Signals {
    pub telemetry_env: Option<String>,
    pub do_not_track: Option<String>,
    pub ci: bool,
    pub sandbox: bool,
    pub interactive: bool,
    /// `STASHBASE_TELEMETRY_DEBUG=1`: print the event instead of sending it.
    pub debug: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Off,
    ShowNotice,
    Send,
    /// Print the event to stderr and send nothing. Works before the notice
    /// has been shown, so anyone can inspect exactly what would be sent.
    DebugPrint,
}

fn env_flag(name: &str) -> bool {
    match std::env::var(name) {
        Ok(value) => {
            let value = value.trim().to_ascii_lowercase();
            !value.is_empty() && value != "0" && value != "false"
        }
        Err(_) => false,
    }
}

impl Signals {
    pub fn from_process() -> Self {
        Self {
            telemetry_env: std::env::var("STASHBASE_TELEMETRY").ok(),
            do_not_track: std::env::var("DO_NOT_TRACK").ok(),
            ci: CI_VARS.iter().any(|name| env_flag(name)),
            sandbox: env_flag("STASHBASE_SANDBOX"),
            interactive: std::io::stdin().is_terminal() && std::io::stderr().is_terminal(),
            debug: std::env::var("STASHBASE_TELEMETRY_DEBUG").is_ok_and(|v| v.trim() == "1"),
        }
    }
}

/// Why telemetry is switched off, or `None` if nothing disables it. The
/// first-run notice is handled separately by `decide`.
pub fn off_reason(signals: &Signals, state: &State) -> Option<&'static str> {
    if signals.sandbox {
        return Some("running inside a sandbox");
    }
    if signals.ci {
        return Some("running in CI");
    }
    if let Some(value) = &signals.telemetry_env {
        if matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "0" | "false" | "off" | "no"
        ) {
            return Some("STASHBASE_TELEMETRY is set to disable it");
        }
    }
    if let Some(value) = &signals.do_not_track {
        let value = value.trim().to_ascii_lowercase();
        if !value.is_empty() && value != "0" && value != "false" {
            return Some("DO_NOT_TRACK is set");
        }
    }
    if state.enabled == Some(false) {
        return Some("disabled with `stashbase telemetry disable`");
    }
    None
}

pub fn decide(signals: &Signals, state: &State) -> Decision {
    if off_reason(signals, state).is_some() {
        return Decision::Off;
    }
    if signals.debug {
        return Decision::DebugPrint;
    }
    if !state.notice_shown {
        return if signals.interactive {
            Decision::ShowNotice
        } else {
            Decision::Off
        };
    }
    Decision::Send
}

pub fn write_notice(out: &mut impl Write) {
    let _ = writeln!(
        out,
        "\nStashbase collects privacy-preserving telemetry to improve the product. It never\n\
         collects commands or arguments, paths, secrets, hosts, or policy contents, and\n\
         nothing is sent from sandboxes or CI. What is sent: docs/telemetry.md\n\
         Disable: stashbase telemetry disable (or STASHBASE_TELEMETRY=0, DO_NOT_TRACK=1).\n"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telemetry::state::State;

    fn seen() -> State {
        State {
            notice_shown: true,
            ..State::default()
        }
    }

    fn human() -> Signals {
        Signals {
            interactive: true,
            ..Signals::default()
        }
    }

    #[test]
    fn sends_after_notice_for_an_interactive_human() {
        assert_eq!(decide(&human(), &seen()), Decision::Send);
    }

    #[test]
    fn first_interactive_run_shows_notice_and_sends_nothing() {
        assert_eq!(decide(&human(), &State::default()), Decision::ShowNotice);
    }

    #[test]
    fn non_interactive_run_never_shows_notice_or_sends_before_it_was_seen() {
        let piped = Signals::default();
        assert_eq!(decide(&piped, &State::default()), Decision::Off);
    }

    #[test]
    fn non_interactive_run_may_send_after_a_human_saw_the_notice() {
        assert_eq!(decide(&Signals::default(), &seen()), Decision::Send);
    }

    #[test]
    fn env_disable_values_are_case_and_whitespace_insensitive() {
        for value in ["0", "false", "FALSE", " False ", "off", "Off", "no", "NO"] {
            let signals = Signals {
                telemetry_env: Some(value.to_string()),
                ..human()
            };
            assert_eq!(decide(&signals, &seen()), Decision::Off, "{value:?}");
        }
    }

    #[test]
    fn do_not_track_disables_unless_zero_or_false() {
        for (value, off) in [("1", true), ("true", true), ("yes", true), ("0", false), ("false", false), ("", false)] {
            let signals = Signals {
                do_not_track: Some(value.to_string()),
                ..human()
            };
            let expected = if off { Decision::Off } else { Decision::Send };
            assert_eq!(decide(&signals, &seen()), expected, "DO_NOT_TRACK={value:?}");
        }
    }

    #[test]
    fn ci_and_sandbox_always_disable() {
        for signals in [
            Signals { ci: true, ..human() },
            Signals { sandbox: true, ..human() },
        ] {
            assert_eq!(decide(&signals, &seen()), Decision::Off);
            assert_eq!(decide(&signals, &State::default()), Decision::Off);
        }
    }

    #[test]
    fn config_opt_out_disables() {
        let state = State {
            enabled: Some(false),
            notice_shown: true,
            ..State::default()
        };
        assert_eq!(decide(&human(), &state), Decision::Off);
        assert_eq!(off_reason(&human(), &state), Some("disabled with `stashbase telemetry disable`"));
    }

    #[test]
    fn notice_names_the_opt_outs_and_what_is_never_collected() {
        let mut out = Vec::new();
        write_notice(&mut out);
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("stashbase telemetry disable"));
        assert!(text.contains("STASHBASE_TELEMETRY=0"));
        assert!(text.contains("DO_NOT_TRACK=1"));
        assert!(text.contains("never"));
    }

    #[test]
    fn debug_mode_prints_even_before_the_notice_and_never_sends() {
        for interactive in [true, false] {
            let signals = Signals {
                debug: true,
                interactive,
                ..Signals::default()
            };
            assert_eq!(decide(&signals, &State::default()), Decision::DebugPrint);
            assert_eq!(decide(&signals, &seen()), Decision::DebugPrint);
        }
    }

    #[test]
    fn debug_mode_still_respects_every_off_switch() {
        let debug = Signals {
            debug: true,
            ..human()
        };
        for signals in [
            Signals { ci: true, ..debug.clone() },
            Signals { sandbox: true, ..debug.clone() },
            Signals { telemetry_env: Some("0".into()), ..debug.clone() },
            Signals { do_not_track: Some("1".into()), ..debug.clone() },
        ] {
            assert_eq!(decide(&signals, &seen()), Decision::Off);
        }
        let opted_out = State {
            enabled: Some(false),
            ..State::default()
        };
        assert_eq!(decide(&debug, &opted_out), Decision::Off);
    }
}
