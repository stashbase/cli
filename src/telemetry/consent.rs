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

/// Variables the agent proxy points at its temporary CA for the child it
/// launches, so intercepted HTTPS is trusted.
const PROXY_CA_VARS: &[&str] = &[
    "SSL_CERT_FILE",
    "CURL_CA_BUNDLE",
    "GIT_SSL_CAINFO",
    "NODE_EXTRA_CA_CERTS",
    "CODEX_CA_CERTIFICATE",
];

/// Whether `path` names a CA file written by the agent proxy:
/// `stashbase-proxy-ca-<id>.pem` (local session) or `remote-proxy-<key>.pem`
/// (remote session). Only the file name is looked at.
pub fn is_stashbase_proxy_ca(path: &str) -> bool {
    let name = std::path::Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    name.ends_with(".pem")
        && (name.starts_with("stashbase-proxy-ca-") || name.starts_with("remote-proxy-"))
}

#[derive(Debug, Default, Clone)]
pub struct Signals {
    pub telemetry_env: Option<String>,
    pub do_not_track: Option<String>,
    pub ci: bool,
    pub sandbox: bool,
    /// The environment carries the agent proxy's CA files: an agent session
    /// even if the `STASHBASE_SANDBOX` marker was scrubbed. A second signal,
    /// independent of that variable, so a harness that strips unknown
    /// variables does not switch the suppression off.
    pub proxied_session: bool,
    /// `STASHBASE_API_URL` (or the build) points somewhere other than
    /// Stashbase's own service: self-hosted or staging.
    pub custom_api_url: bool,
    /// `STASHBASE_TELEMETRY_URL` is set: sending elsewhere is explicit.
    pub telemetry_url: bool,
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
            // Presence is enough: any value, including "0", counts, so setting
            // it to a false-looking value does not switch suppression off.
            sandbox: std::env::var_os("STASHBASE_SANDBOX").is_some(),
            custom_api_url: !crate::api::client::is_default_api_url(),
            telemetry_url: std::env::var(super::send::TELEMETRY_URL_ENV)
                .is_ok_and(|value| !value.trim().is_empty()),
            proxied_session: PROXY_CA_VARS
                .iter()
                .any(|name| std::env::var(name).is_ok_and(|path| is_stashbase_proxy_ca(&path))),
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
    if signals.proxied_session {
        return Some("running inside an agent proxy session");
    }
    // Events only go to Stashbase's own service, like `gh` skips GitHub
    // Enterprise Server. Setting STASHBASE_TELEMETRY_URL is the explicit opt-in
    // to send somewhere else.
    if signals.custom_api_url && !signals.telemetry_url {
        return Some("the API URL is not Stashbase's own service");
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
        return Some("opted out with `stashbase telemetry disable`");
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
        "\nStashbase collects privacy-preserving telemetry: which command ran and whether it\n\
         succeeded, never arguments, paths, secrets or hosts. Details: docs/telemetry.md\n\
         Disable: stashbase telemetry disable (or STASHBASE_TELEMETRY=0, DO_NOT_TRACK=1)\n"
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
        assert_eq!(off_reason(&human(), &state), Some("opted out with `stashbase telemetry disable`"));
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
        assert!(text.contains("docs/telemetry.md"));
    }

    #[test]
    fn notice_is_short_and_does_not_overclaim() {
        let mut out = Vec::new();
        write_notice(&mut out);
        let text = String::from_utf8(out).unwrap();
        let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
        assert!(lines.len() <= 3, "{} lines:\n{text}", lines.len());
        assert!(lines.iter().all(|l| l.len() <= 100), "{text}");
        // The command that ran is reported (setup, pull, ...); only its
        // arguments are not. Do not claim otherwise.
        assert!(!text.contains("never collects commands"), "{text}");
        assert!(text.contains("arguments"), "{text}");
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

    #[test]
    fn recognises_the_agent_proxy_ca_files_by_name() {
        for yes in [
            "/tmp/stashbase-proxy-ca-0c9d1f2e-7a1b-4c3d-8e9f-001122334455.pem",
            "/var/folders/ab/cd/T/stashbase-proxy-ca-x.pem",
            "/home/u/.cache/stashbase/remote-proxy-abc123.pem",
        ] {
            assert!(is_stashbase_proxy_ca(yes), "{yes}");
        }
        for no in [
            "",
            "/etc/ssl/cert.pem",
            "/etc/ssl/certs/ca-certificates.crt",
            "/tmp/stashbase-proxy-ca-x.txt",
            "/tmp/my-stashbase-proxy-ca-x.pem",
            "/tmp/remote-proxy.pem",
        ] {
            assert!(!is_stashbase_proxy_ca(no), "{no:?}");
        }
    }

    #[test]
    fn a_proxied_agent_session_is_an_off_switch_even_without_the_marker() {
        let proxied = Signals {
            proxied_session: true,
            ..human()
        };
        assert_eq!(decide(&proxied, &seen()), Decision::Off);
        assert_eq!(decide(&proxied, &State::default()), Decision::Off);
        let debug = Signals { debug: true, ..proxied.clone() };
        assert_eq!(decide(&debug, &seen()), Decision::Off);
        assert_eq!(
            off_reason(&proxied, &seen()),
            Some("running inside an agent proxy session")
        );
    }

    #[test]
    fn a_non_default_api_url_turns_telemetry_off_unless_the_destination_is_explicit() {
        let custom = Signals {
            custom_api_url: true,
            ..human()
        };
        assert_eq!(decide(&custom, &seen()), Decision::Off);
        assert_eq!(decide(&custom, &State::default()), Decision::Off);
        assert_eq!(
            off_reason(&custom, &seen()),
            Some("the API URL is not Stashbase's own service")
        );
        // Debug mode does not override it: nothing would be sent anyway.
        let debug = Signals { debug: true, ..custom.clone() };
        assert_eq!(decide(&debug, &seen()), Decision::Off);

        // An explicit telemetry URL is the opt-in to send somewhere else.
        let explicit = Signals {
            telemetry_url: true,
            ..custom
        };
        assert_eq!(decide(&explicit, &seen()), Decision::Send);
    }
}
