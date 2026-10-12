//! Checks for `[config_env]`, the profile's plaintext environment for the
//! agent: which names it may not set, and which values look like credentials
//! that belong in `[secrets]` instead.

use std::io::IsTerminal;

use crate::models::agent::AgentProfile;

/// Variables the sandbox itself sets or strips. Letting a profile set one
/// would break the session or weaken its containment (proxy routing, CA
/// trust, broker access). Compared case-insensitively, since tools read both
/// `HTTP_PROXY` and `http_proxy`. Everything prefixed `STASHBASE_` is
/// reserved too, see [`reserved_env_name`].
const RESERVED_CHILD_ENV: &[&str] = &[
    "HOME",
    "TMPDIR",
    "FORCE_COLOR",
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "ALL_PROXY",
    "NO_PROXY",
    "NODE_USE_ENV_PROXY",
    "NPM_CONFIG_PROXY",
    "NPM_CONFIG_HTTPS_PROXY",
    "NPM_CONFIG_NOPROXY",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
    "CURL_CA_BUNDLE",
    "GIT_SSL_CAINFO",
    "NODE_EXTRA_CA_CERTS",
    "REQUESTS_CA_BUNDLE",
    "CODEX_CA_CERTIFICATE",
];

/// Largest `NAME=value` entry `[config_env]` accepts. Linux rejects any
/// single environment string over 128 KiB at spawn (`E2BIG`); config is far
/// smaller, and anything this big is a file that belongs in the workspace.
pub(crate) const MAX_CONFIG_ENV_ENTRY_BYTES: usize = 64 * 1024;
/// Largest `[config_env]` in total. Arguments and the whole environment share
/// one spawn limit (about 1 MB on macOS, 2 MB on Linux), and the Docker
/// backend also passes every value on the `docker` command line.
pub(crate) const MAX_CONFIG_ENV_TOTAL_BYTES: usize = 256 * 1024;

/// Size of the `NAME=value` string the child's environment holds.
pub(crate) fn config_env_entry_bytes(name: &str, value: &str) -> usize {
    name.len() + 1 + value.len()
}

/// Why `[config_env]` must not set `name`, or `None` when it may.
///
/// Besides names the sandbox owns, this rejects names that change which
/// program or libraries the native launcher loads. `[config_env]` is applied
/// to the launcher process (`bwrap`, `systemd-run`) before any confinement
/// exists, and the launcher is found through the child's `PATH`, so a
/// repository profile setting `PATH` or `LD_PRELOAD` would run its own code
/// outside the sandbox.
pub(crate) fn reserved_env_reason(name: &str) -> Option<&'static str> {
    let upper = name.to_ascii_uppercase();
    if upper.starts_with("STASHBASE_")
        || RESERVED_CHILD_ENV
            .iter()
            .any(|reserved| reserved.eq_ignore_ascii_case(name))
    {
        return Some("is set by the Stashbase sandbox and can't be overridden");
    }
    if upper == "PATH" || upper.starts_with("LD_") || upper.starts_with("DYLD_") {
        return Some(
            "controls which programs and libraries start the sandbox, so a profile can't set it",
        );
    }
    None
}

/// The binding that already provides `name` to the child, as a
/// `[secrets.X]` / `[personal_credentials.X]` label. A binding's exposed
/// `env` name carries the proxy placeholder, and a renamed source (`from`)
/// is stripped from the child on purpose, so neither may be set as plaintext.
pub(crate) fn conflicting_binding(profile: &AgentProfile, name: &str) -> Option<String> {
    let tables = [
        ("secrets", &profile.secrets.bindings),
        ("personal_credentials", &profile.personal_credentials),
    ];
    for (table, bindings) in tables {
        let mut binding_names = bindings.keys().collect::<Vec<_>>();
        binding_names.sort();
        for binding_name in binding_names {
            let binding = &bindings[binding_name];
            let env = binding.env.as_deref().unwrap_or(binding_name);
            let from = binding.from.as_deref().unwrap_or(binding_name);
            // Case-insensitive: Windows treats `github_token` and
            // `GITHUB_TOKEN` as the same variable.
            if env.eq_ignore_ascii_case(name) || from.eq_ignore_ascii_case(name) {
                return Some(format!("[{table}.{binding_name}]"));
            }
        }
    }
    None
}

const CREDENTIAL_NAME_PARTS: &[&str] = &[
    "TOKEN",
    "SECRET",
    "PASSWORD",
    "PASSWD",
    "API_KEY",
    "APIKEY",
    "PRIVATE_KEY",
    "CREDENTIAL",
];

const CREDENTIAL_PREFIXES: &[&str] = &[
    "ghp_",
    "gho_",
    "ghs_",
    "ghu_",
    "github_pat_",
    "sk-",
    "xoxa-",
    "xoxb-",
    "xoxp-",
    "xoxr-",
    "AKIA",
    "glpat-",
    "-----BEGIN",
];

/// Why `value` (set as `name`) looks like a credential, or `None` when it
/// looks like ordinary config. A heuristic: it errs toward flagging, and a
/// profile acknowledges a false positive with `not_secret = true`.
pub(crate) fn secret_likeness(name: &str, value: &str) -> Option<&'static str> {
    let upper_name = name.to_ascii_uppercase();
    if CREDENTIAL_NAME_PARTS
        .iter()
        .any(|part| upper_name.contains(part))
    {
        return Some("its name looks like a credential");
    }
    let value = value.trim();
    if CREDENTIAL_PREFIXES
        .iter()
        .any(|prefix| value.starts_with(prefix))
    {
        return Some("its value matches a known credential format");
    }
    if url_has_password(value) {
        return Some("its value is a URL containing a password");
    }
    if high_entropy(value) {
        return Some("its value looks randomly generated, like a key or token");
    }
    None
}

fn url_has_password(value: &str) -> bool {
    reqwest::Url::parse(value).is_ok_and(|url| url.password().is_some_and(|p| !p.is_empty()))
}

fn high_entropy(value: &str) -> bool {
    if value.len() < 20
        || value
            .chars()
            .any(|ch| ch.is_whitespace() || ch == '/' || !ch.is_ascii())
        || is_uuid(value)
    {
        return false;
    }
    let entropy = shannon_entropy(value);
    if value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        value.len() >= 32 && entropy >= 3.0
    } else {
        entropy >= 4.0
    }
}

fn is_uuid(value: &str) -> bool {
    let groups = value.split('-').collect::<Vec<_>>();
    groups.len() == 5
        && groups
            .iter()
            .zip([8, 4, 4, 4, 12])
            .all(|(group, len)| group.len() == len && group.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// Shannon entropy in bits per byte.
fn shannon_entropy(value: &str) -> f64 {
    let mut counts = [0usize; 256];
    for byte in value.bytes() {
        counts[byte as usize] += 1;
    }
    let len = value.len() as f64;
    counts
        .iter()
        .filter(|&&count| count > 0)
        .map(|&count| {
            let probability = count as f64 / len;
            -probability * probability.log2()
        })
        .sum()
}

/// `[config_env]` entries that look like credentials and have not been
/// acknowledged with `not_secret = true`, as `(name, reason)` in name order.
pub(crate) fn flagged_config_env(profile: &AgentProfile) -> Vec<(String, &'static str)> {
    profile
        .config_env
        .iter()
        .filter(|(_, value)| !value.acknowledged())
        .filter_map(|(name, value)| {
            secret_likeness(name, &value.value()).map(|reason| (name.clone(), reason))
        })
        .collect()
}

pub(crate) const NOT_SECRET_HINT: &str =
    "Move credentials to [secrets], or confirm a value is safe with `NAME = { value = \"...\", not_secret = true }`.";

/// Before a session starts, asks the user to confirm `[config_env]` values
/// that look like credentials. They would be handed to the agent in
/// plaintext, so a run that can't ask (CI, piped stdin, `--silent`) fails
/// instead. Values are never printed.
pub fn confirm_config_env(profile: &AgentProfile, silent: bool) -> anyhow::Result<()> {
    let flagged = flagged_config_env(profile);
    if flagged.is_empty() {
        return Ok(());
    }
    let list = flagged
        .iter()
        .map(|(name, reason)| format!("- {name}: {reason}"))
        .collect::<Vec<_>>()
        .join("\n");
    let interactive = !silent && std::io::stdin().is_terminal() && std::io::stderr().is_terminal();
    if !interactive {
        anyhow::bail!(
            "[config_env] has values that look like credentials:\n{list}\n{NOT_SECRET_HINT}"
        );
    }
    eprintln!("[config_env] has values that look like credentials:\n{list}\n{NOT_SECRET_HINT}");
    let proceed = crate::utils::interaction::confirm_opt(
        "Pass these values to the agent in plaintext anyway?",
    )
    .unwrap_or(false);
    // See ensure_docker_sandbox_image_available: dialoguer can leave the
    // cursor hidden when the prompt is dismissed with Ctrl+C.
    let _ = dialoguer::console::Term::stdout().show_cursor();
    if !proceed {
        anyhow::bail!("agent run cancelled: [config_env] values were not confirmed");
    }
    // Separate the answered prompt from the session output that follows.
    eprintln!();
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::models::agent::{
        AgentBindingProfile, AgentEnvScalar, AgentEnvValue, AgentEnvValueDetails,
    };

    #[test]
    fn flags_credential_names() {
        for name in ["GITHUB_TOKEN", "db_password", "OPENAI_API_KEY", "MY_SECRET"] {
            assert!(secret_likeness(name, "x").is_some(), "{name}");
        }
    }

    #[test]
    fn flags_known_credential_formats() {
        for value in [
            "ghp_abc",
            "github_pat_11ABC",
            "sk-ant-api03-xyz",
            "xoxb-123",
            "AKIAIOSFODNN7EXAMPLE",
            "glpat-abc",
            "-----BEGIN OPENSSH PRIVATE KEY-----",
        ] {
            assert!(secret_likeness("VALUE", value).is_some(), "{value}");
        }
    }

    #[test]
    fn flags_urls_with_passwords() {
        assert!(secret_likeness("DATABASE_URL", "postgres://app:hunter2@db:5432/app").is_some());
        assert!(secret_likeness("DATABASE_URL", "postgres://app@localhost/app").is_none());
        assert!(secret_likeness("API_BASE", "https://api.example.com/v1").is_none());
    }

    #[test]
    fn flags_high_entropy_values() {
        assert!(secret_likeness("VALUE", "Zq8vT2mXk4Lr9Wp1Ys6Bn3Hd").is_some());
        assert!(secret_likeness("VALUE", "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b").is_some());
    }

    #[test]
    fn leaves_ordinary_config_alone() {
        for value in [
            "development",
            "info,my_crate=debug",
            "1.2.3",
            "3000",
            "true",
            "/usr/local/share/data",
            "123e4567-e89b-12d3-a456-426614174000",
            "deadbeef",
            "some sentence with several words in it",
        ] {
            assert!(secret_likeness("VALUE", value).is_none(), "{value}");
        }
    }

    #[test]
    fn reserves_sandbox_owned_names_case_insensitively() {
        for name in [
            "HTTPS_PROXY",
            "https_proxy",
            "SSL_CERT_FILE",
            "HOME",
            "STASHBASE_API_KEY",
            "stashbase_sandbox",
        ] {
            assert_eq!(
                reserved_env_reason(name),
                Some("is set by the Stashbase sandbox and can't be overridden"),
                "{name}"
            );
        }
        assert_eq!(reserved_env_reason("NODE_ENV"), None);
    }

    #[test]
    fn reserves_names_that_change_how_the_launcher_starts() {
        for name in [
            "PATH",
            "path",
            "LD_PRELOAD",
            "LD_LIBRARY_PATH",
            "LD_AUDIT",
            "ld_preload",
            "DYLD_INSERT_LIBRARIES",
            "DYLD_LIBRARY_PATH",
        ] {
            assert!(
                reserved_env_reason(name)
                    .is_some_and(|reason| reason.contains("start the sandbox")),
                "{name}"
            );
        }
        for name in ["PATHS", "MY_PATH", "OLD_PATH", "COLD_START"] {
            assert_eq!(reserved_env_reason(name), None, "{name}");
        }
    }

    fn profile_with(
        bindings: HashMap<String, AgentBindingProfile>,
        personal: HashMap<String, AgentBindingProfile>,
    ) -> AgentProfile {
        let mut profile: AgentProfile = toml::from_str("").unwrap();
        profile.secrets = bindings.into();
        profile.personal_credentials = personal;
        profile
    }

    fn binding(from: Option<&str>, env: Option<&str>) -> AgentBindingProfile {
        let mut binding: AgentBindingProfile = toml::from_str("").unwrap();
        binding.from = from.map(str::to_owned);
        binding.env = env.map(str::to_owned);
        binding
    }

    #[test]
    fn finds_conflicting_bindings() {
        let profile = profile_with(
            HashMap::from([
                ("GITHUB_TOKEN".to_owned(), binding(None, None)),
                (
                    "NPM".to_owned(),
                    binding(Some("NPM_SOURCE"), Some("NPM_TOKEN")),
                ),
            ]),
            HashMap::from([("LINEAR".to_owned(), binding(None, Some("LINEAR_API_KEY")))]),
        );
        assert_eq!(
            conflicting_binding(&profile, "GITHUB_TOKEN").as_deref(),
            Some("[secrets.GITHUB_TOKEN]")
        );
        assert_eq!(
            conflicting_binding(&profile, "NPM_TOKEN").as_deref(),
            Some("[secrets.NPM]")
        );
        assert_eq!(
            conflicting_binding(&profile, "NPM_SOURCE").as_deref(),
            Some("[secrets.NPM]")
        );
        assert_eq!(
            conflicting_binding(&profile, "LINEAR_API_KEY").as_deref(),
            Some("[personal_credentials.LINEAR]")
        );
        assert_eq!(
            conflicting_binding(&profile, "github_token").as_deref(),
            Some("[secrets.GITHUB_TOKEN]")
        );
        assert_eq!(conflicting_binding(&profile, "NODE_ENV"), None);
    }

    #[test]
    fn acknowledged_values_are_not_flagged() {
        let mut profile = profile_with(HashMap::new(), HashMap::new());
        profile.config_env.insert(
            "BUILD_HASH".to_owned(),
            AgentEnvValue::Detailed(AgentEnvValueDetails {
                value: AgentEnvScalar::String(
                    "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b".to_owned(),
                ),
                not_secret: true,
            }),
        );
        profile.config_env.insert(
            "DEPLOY_TOKEN".to_owned(),
            AgentEnvValue::Plain(AgentEnvScalar::String("abc".to_owned())),
        );
        assert_eq!(
            flagged_config_env(&profile),
            vec![(
                "DEPLOY_TOKEN".to_owned(),
                "its name looks like a credential"
            )]
        );
    }

    #[test]
    fn parses_both_value_forms_and_rejects_unknown_fields() {
        let profile: AgentProfile = toml::from_str(
            "[config_env]\nNODE_ENV = \"development\"\nBUILD = { value = \"abc\", not_secret = true }\n",
        )
        .unwrap();
        assert_eq!(profile.config_env["NODE_ENV"].value(), "development");
        assert!(!profile.config_env["NODE_ENV"].acknowledged());
        assert_eq!(profile.config_env["BUILD"].value(), "abc");
        assert!(profile.config_env["BUILD"].acknowledged());

        assert!(toml::from_str::<AgentProfile>(
            "[config_env]\nBUILD = { value = \"abc\", typo = true }\n"
        )
        .is_err());
    }

    #[test]
    fn accepts_toml_scalars_as_strings() {
        let profile: AgentProfile = toml::from_str(
            "[config_env]\nPORT = 3000\nDEBUG = true\nRATIO = 1.0\nSCALE = 0.25\nLEGACY = { value = 8080, not_secret = true }\n",
        )
        .unwrap();
        assert_eq!(profile.config_env["PORT"].value(), "3000");
        assert_eq!(profile.config_env["DEBUG"].value(), "true");
        assert_eq!(profile.config_env["RATIO"].value(), "1.0");
        assert_eq!(profile.config_env["SCALE"].value(), "0.25");
        assert_eq!(profile.config_env["LEGACY"].value(), "8080");
        assert!(profile.config_env["LEGACY"].acknowledged());

        assert!(toml::from_str::<AgentProfile>("[config_env]\nLIST = [1, 2]\n").is_err());
    }
}
