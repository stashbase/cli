//! Checks for `[config_env]`, the profile's plaintext environment for the
//! agent: which names it may not set, and which values look sensitive
//! that belong in `[secrets]` instead.

use std::io::IsTerminal;

use crate::models::agent::{AgentEnvScalar, AgentEnvValue, AgentProfile, SandboxBackend};

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

/// Why `value` (set as `name`) looks sensitive, or `None` when it
/// looks like ordinary config. A heuristic: it errs toward flagging, and a
/// profile acknowledges a false positive with `not_sensitive = true`.
pub(crate) fn secret_likeness(name: &str, value: &str) -> Option<&'static str> {
    // An empty value holds nothing, and `NAME = ""` is how a profile blanks a
    // credential the agent would otherwise inherit from the shell.
    if value.trim().is_empty() {
        return None;
    }
    if let Some(reason) = credential_likeness(name, value) {
        return Some(reason);
    }
    if high_entropy(value.trim()) {
        return Some("its value looks randomly generated, like a key or token");
    }
    None
}

/// [`secret_likeness`] without the entropy check, which is too noisy over a
/// whole shell environment (session ids, hashes, terminal cookies).
fn credential_likeness(name: &str, value: &str) -> Option<&'static str> {
    let upper_name = name.to_ascii_uppercase();
    if CREDENTIAL_NAME_PARTS
        .iter()
        .any(|part| upper_name.contains(part))
    {
        return Some("its name looks sensitive");
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

/// `[config_env]` entries that look sensitive and have not been
/// acknowledged with `not_sensitive = true`, as `(name, reason)` in name order.
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

/// Shown in place of a `[config_env]` value that looks sensitive.
pub(crate) const MASKED_CONFIG_ENV_VALUE: &str = "<hidden: looks sensitive>";

/// Replaces each flagged, unacknowledged `[config_env]` value with
/// [`MASKED_CONFIG_ENV_VALUE`], so printing a profile doesn't show what the
/// check exists to keep from being passed around unnoticed.
pub(crate) fn mask_flagged_config_env(profile: &mut AgentProfile) {
    for (name, _) in flagged_config_env(profile) {
        profile.config_env.insert(
            name,
            AgentEnvValue::Plain(AgentEnvScalar::String(MASKED_CONFIG_ENV_VALUE.to_owned())),
        );
    }
}

pub(crate) const NOT_SENSITIVE_HINT: &str =
    "Move sensitive values to [secrets], or confirm a value is safe with `NAME = { value = \"...\", not_sensitive = true }`.";

/// Before a session starts, asks the user to confirm `[config_env]` values
/// that look sensitive. They would be handed to the agent in
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
        anyhow::bail!("[config_env] has values that look sensitive:\n{list}\n{NOT_SENSITIVE_HINT}");
    }
    eprintln!("[config_env] has values that look sensitive:\n{list}\n{NOT_SENSITIVE_HINT}");
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

/// Credential-looking variables in `inherited` (the environment `agent run`
/// was started from) that a native-backend agent would inherit, sorted by
/// name. Best-effort: oddly named credentials and capabilities such as
/// `SSH_AUTH_SOCK` are not recognized. Skips `STASHBASE_API_KEY` (the
/// launcher removes it), names the sandbox sets, names a binding or
/// `[config_env]` provides, and values that are paths (`GIT_ASKPASS`,
/// `*_TOKEN_FILE`), which point at a credential rather than hold one.
/// Other `STASHBASE_*` names are reserved for profiles but still inherited,
/// so they are checked like any other variable.
pub(crate) fn credential_looking_inherited_env(
    profile: &AgentProfile,
    inherited: impl IntoIterator<Item = (String, String)>,
) -> Vec<(String, &'static str)> {
    let provided = |name: &str| {
        name.eq_ignore_ascii_case("STASHBASE_API_KEY")
            || RESERVED_CHILD_ENV
                .iter()
                .any(|reserved| reserved.eq_ignore_ascii_case(name))
            || conflicting_binding(profile, name).is_some()
            || profile
                .config_env
                .keys()
                .any(|other| other.eq_ignore_ascii_case(name))
    };
    let mut flagged = inherited
        .into_iter()
        .filter(|(_, value)| {
            let value = value.trim();
            !value.is_empty() && !value.starts_with('/') && !value.starts_with("~/")
        })
        .filter(|(name, _)| !provided(name))
        .filter_map(|(name, value)| credential_likeness(&name, &value).map(|reason| (name, reason)))
        .collect::<Vec<_>>();
    flagged.sort();
    flagged
}

/// Prints one line about credential-looking variables a native run would
/// pass to the agent. `agent validate` lists them by name. Docker runs start
/// from a clean environment, so they are skipped. Values are never printed.
pub fn warn_inherited_env(profile: &AgentProfile, profile_name: &str, silent: bool) {
    if silent || profile.sandbox.backend == SandboxBackend::Docker {
        return;
    }
    let count = credential_looking_inherited_env(profile, std::env::vars()).len();
    if count > 0 {
        eprintln!(
            "Warning: {count} credential-looking shell variable(s) are visible to the agent. See `stashbase agent validate --profile {profile_name}`."
        );
    }
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
    fn empty_values_are_not_sensitive() {
        assert_eq!(secret_likeness("GITHUB_TOKEN", ""), None);
        assert_eq!(secret_likeness("GITHUB_TOKEN", "  "), None);
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
                not_sensitive: true,
            }),
        );
        profile.config_env.insert(
            "DEPLOY_TOKEN".to_owned(),
            AgentEnvValue::Plain(AgentEnvScalar::String("abc".to_owned())),
        );
        assert_eq!(
            flagged_config_env(&profile),
            vec![("DEPLOY_TOKEN".to_owned(), "its name looks sensitive")]
        );
    }

    #[test]
    fn parses_both_value_forms_and_rejects_unknown_fields() {
        let profile: AgentProfile = toml::from_str(
            "[config_env]\nNODE_ENV = \"development\"\nBUILD = { value = \"abc\", not_sensitive = true }\n",
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
            "[config_env]\nPORT = 3000\nDEBUG = true\nRATIO = 1.0\nSCALE = 0.25\nLEGACY = { value = 8080, not_sensitive = true }\n",
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

    #[test]
    fn masks_only_flagged_unacknowledged_values() {
        let mut profile: AgentProfile = toml::from_str(
            "[config_env]\nNODE_ENV = \"development\"\nGH_PAT = \"ghp_abc\"\nBUILD = { value = \"9f86d081884c7d659a2feaa0c55ad015a3bf4f1b\", not_sensitive = true }\n",
        )
        .unwrap();

        mask_flagged_config_env(&mut profile);

        assert_eq!(profile.config_env["NODE_ENV"].value(), "development");
        assert_eq!(
            profile.config_env["GH_PAT"].value(),
            MASKED_CONFIG_ENV_VALUE
        );
        assert_eq!(
            profile.config_env["BUILD"].value(),
            "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b"
        );
    }

    fn inherited(vars: &[(&str, &str)]) -> Vec<(String, String)> {
        vars.iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect()
    }

    fn flagged_names(profile: &AgentProfile, vars: &[(&str, &str)]) -> Vec<String> {
        credential_looking_inherited_env(profile, inherited(vars))
            .into_iter()
            .map(|(name, _)| name)
            .collect()
    }

    #[test]
    fn flags_inherited_credentials_by_name_and_format_but_not_entropy() {
        let profile: AgentProfile = toml::from_str("").unwrap();
        let names = flagged_names(
            &profile,
            &[
                ("OPENAI_API_KEY", "x"),
                ("DEPLOY", "ghp_abcdefghijklmnopqrstuvwxyz0123456789"),
                ("DATABASE_URL", "postgres://app:hunter2@db:5432/app"),
                (
                    "TERM_SESSION_ID",
                    "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b",
                ),
                ("EDITOR", "nvim"),
            ],
        );
        assert_eq!(names, ["DATABASE_URL", "DEPLOY", "OPENAI_API_KEY"]);
    }

    #[test]
    fn skips_inherited_paths_empty_values_and_the_removed_api_key() {
        let profile: AgentProfile = toml::from_str("").unwrap();
        let names = flagged_names(
            &profile,
            &[
                ("GH_TOKEN_FILE", "/run/secrets/gh"),
                ("NPM_TOKEN_PATH", "~/.npm-token"),
                ("EMPTY_TOKEN", ""),
                ("STASHBASE_API_KEY", "sb_live"),
                ("STASHBASE_CUSTOM_TOKEN", "x"),
            ],
        );
        assert_eq!(names, ["STASHBASE_CUSTOM_TOKEN"]);
    }

    #[test]
    fn skips_inherited_names_the_profile_provides() {
        let mut profile = profile_with(
            HashMap::from([
                ("GITHUB_TOKEN".to_owned(), binding(None, None)),
                (
                    "NPM".to_owned(),
                    binding(Some("NPM_SOURCE_TOKEN"), Some("NPM_TOKEN")),
                ),
            ]),
            HashMap::new(),
        );
        profile.config_env.insert(
            "PUBLIC_TOKEN".to_owned(),
            AgentEnvValue::Plain(AgentEnvScalar::String("x".to_owned())),
        );
        let names = flagged_names(
            &profile,
            &[
                ("GITHUB_TOKEN", "x"),
                ("NPM_SOURCE_TOKEN", "x"),
                ("NPM_TOKEN", "x"),
                ("public_token", "x"),
                ("AWS_SECRET_ACCESS_KEY", "x"),
            ],
        );
        assert_eq!(names, ["AWS_SECRET_ACCESS_KEY"]);
    }
}
