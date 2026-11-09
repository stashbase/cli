//! Interactive `stashbase agent init`: a short prompt flow that writes a
//! standard, validated agent profile without hand-editing TOML.
//!
//! The prompts only collect [`WizardAnswers`]; [`render_profile`] turns them
//! into TOML and checks it with the same validation `agent run` applies, so
//! the profile-building logic is testable without a terminal.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use dialoguer::{theme::ColorfulTheme, Confirm, Input, Select};
use toml::{Table, Value};

use crate::{handlers::agent::validate::ensure_profile_is_valid, models::agent::AgentProfile};

const READ_METHODS: &[&str] = &["GET", "HEAD"];
const WRITE_METHODS: &[&str] = &["POST", "PUT", "PATCH", "DELETE"];
const PROTECTED_READ: &[&str] = &["~/.ssh", "~/.aws", ".env"];
const PROTECTED_WRITE: &[&str] = &["~/.ssh", "~/.aws", ".git"];
const DOCS_URL: &str = "https://stashbase.dev/docs/agent-profiles";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodingAgent {
    ClaudeCode,
    Codex,
    Other(String),
}

impl CodingAgent {
    pub fn command(&self) -> &str {
        match self {
            Self::ClaudeCode => "claude",
            Self::Codex => "codex",
            Self::Other(command) => command,
        }
    }

    /// Hosts the agent itself needs to reach its model provider.
    pub fn default_hosts(&self) -> Vec<String> {
        let hosts: &[&str] = match self {
            Self::ClaudeCode => &["api.anthropic.com", "platform.claude.com"],
            Self::Codex => &["chatgpt.com", "api.openai.com", "auth.openai.com"],
            Self::Other(_) => &[],
        };
        hosts.iter().map(|host| host.to_string()).collect()
    }

    fn label(&self) -> &str {
        match self {
            Self::ClaudeCode => "Claude Code",
            Self::Codex => "Codex",
            Self::Other(command) => command,
        }
    }
}

/// One secret the agent may use, injected by the proxy only for requests to `host`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretGrant {
    pub name: String,
    pub env: String,
    pub host: String,
    pub allow_writes: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CredentialSource {
    None,
    File {
        path: String,
        secrets: Vec<SecretGrant>,
    },
    Environment {
        project: String,
        environment: String,
        secrets: Vec<SecretGrant>,
    },
}

impl CredentialSource {
    fn secrets(&self) -> &[SecretGrant] {
        match self {
            Self::None => &[],
            Self::File { secrets, .. } | Self::Environment { secrets, .. } => secrets,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WizardAnswers {
    pub agent: CodingAgent,
    pub credentials: CredentialSource,
    pub extra_hosts: Vec<String>,
    pub protect_files: bool,
    pub docker: bool,
}

impl WizardAnswers {
    /// Every host the agent may reach: its own, each credential's, then extras.
    pub fn egress_hosts(&self) -> Vec<String> {
        let mut hosts = self.agent.default_hosts();
        let credential_hosts = self.credentials.secrets().iter().map(|s| s.host.clone());
        for host in credential_hosts.chain(self.extra_hosts.iter().cloned()) {
            if !hosts.contains(&host) {
                hosts.push(host);
            }
        }
        hosts
    }
}

/// Builds the profile TOML for `answers` and checks it with the profile
/// validation `agent run` uses. Never contains secret values, only names.
pub fn render_profile(answers: &WizardAnswers) -> Result<String> {
    let mut root = Table::new();
    match &answers.credentials {
        CredentialSource::File { path, .. } => {
            root.insert("file".into(), Value::String(path.clone()));
        }
        CredentialSource::None | CredentialSource::Environment { .. } => {}
    }
    root.insert("egress_hosts".into(), strings(answers.egress_hosts()));

    if answers.protect_files {
        let mut filesystem = Table::new();
        filesystem.insert("deny_read".into(), strs(PROTECTED_READ));
        filesystem.insert("deny_write".into(), strs(PROTECTED_WRITE));
        root.insert("filesystem".into(), Value::Table(filesystem));
    }

    if answers.docker {
        let mut sandbox = Table::new();
        sandbox.insert("backend".into(), Value::String("docker".into()));
        root.insert("sandbox".into(), Value::Table(sandbox));
    }

    let secrets_list = answers.credentials.secrets();
    if !secrets_list.is_empty() {
        let mut secrets = Table::new();
        if let CredentialSource::Environment {
            project,
            environment,
            ..
        } = &answers.credentials
        {
            secrets.insert("project".into(), Value::String(project.clone()));
            secrets.insert("environment".into(), Value::String(environment.clone()));
        }
        for grant in secrets_list {
            let mut methods: Vec<&str> = READ_METHODS.to_vec();
            if grant.allow_writes {
                methods.extend_from_slice(WRITE_METHODS);
            }
            let mut rule = Table::new();
            rule.insert("effect".into(), Value::String("allow".into()));
            rule.insert("hosts".into(), strings(vec![grant.host.clone()]));
            rule.insert("methods".into(), strs(&methods));
            rule.insert("paths".into(), strs(&["*"]));

            let mut binding = Table::new();
            if grant.env != grant.name {
                binding.insert("env".into(), Value::String(grant.env.clone()));
            }
            binding.insert("rules".into(), Value::Array(vec![Value::Table(rule)]));
            secrets.insert(grant.name.clone(), Value::Table(binding));
        }
        root.insert("secrets".into(), Value::Table(secrets));
    }

    let body = toml::to_string(&root).context("Could not serialize the agent profile.")?;
    let profile: AgentProfile =
        toml::from_str(&body).context("Generated agent profile did not parse.")?;
    ensure_profile_is_valid(&profile)?;

    Ok(format!(
        "# Stashbase Agent Proxy profile, created by `stashbase agent init` for {}.\n\
         # Edit freely; check changes with `stashbase agent validate --profile <name>`.\n\
         # Reference: {DOCS_URL}\n\n{body}{}",
        answers.agent.label(),
        optional_sections(answers)
    ))
}

/// Commented examples of the sections the wizard left out, so the next step
/// is visible when the profile is opened. All stay inert until uncommented.
fn optional_sections(answers: &WizardAnswers) -> String {
    let mut out = String::from("\n# --- Optional: uncomment and edit to extend this profile ---\n");
    if !answers.protect_files {
        out.push_str(
            "\n# Protect sensitive files from agent reads and writes.\n\
             # [filesystem]\n\
             # deny_read = [\"~/.ssh\", \"~/.aws\", \".env\"]\n\
             # deny_write = [\"~/.ssh\", \"~/.aws\", \".git\"]\n",
        );
    }
    if answers.credentials.secrets().is_empty() {
        out.push_str(
            "\n# Grant a secret, sent only on matching requests.\n\
             # file = \".env\"   # or, under [secrets]: project = \"...\", environment = \"...\"\n\
             # [secrets.SECRET_NAME]\n\
             # [[secrets.SECRET_NAME.rules]]\n\
             # effect = \"allow\"\n\
             # hosts = [\"api.example.com\"]\n\
             # methods = [\"GET\"]\n\
             # paths = [\"/v1/*\"]\n",
        );
    }
    out.push_str(
        "\n# Non-sensitive config for the agent, stored in plaintext.\n\
         # [config_env]\n\
         # NODE_ENV = \"development\"\n\
         \n# An HTTP MCP server with a tool allowlist.\n\
         # [mcp_servers.example]\n\
         # url = \"https://mcp.example.com/mcp\"\n\
         # allow_tools = [\"search\"]\n\
         \n# Installed dependency and secret-scan hooks.\n\
         # allow_hooks = [\"dependency_check\", \"secret_scan\"]\n\
         \n# Run the agent in its own git worktree.\n\
         # [workspace]\n\
         # worktree = true\n",
    );
    out
}

fn strings(values: Vec<String>) -> Value {
    Value::Array(values.into_iter().map(Value::String).collect())
}

fn strs(values: &[&str]) -> Value {
    strings(values.iter().map(|value| value.to_string()).collect())
}

// ---------------------------------------------------------------------------
// Prompts
// ---------------------------------------------------------------------------

/// What the wizard decided to write, for the caller to save and report.
pub struct WizardOutcome {
    pub profile: String,
    pub path: PathBuf,
    pub global: bool,
    pub contents: String,
    pub agent_command: String,
    pub overwrite: bool,
}

pub struct WizardTargets<'a> {
    pub preset_global: bool,
    pub force: bool,
    pub resolve: &'a dyn Fn(&str, bool) -> Result<PathBuf>,
    pub display: &'a dyn Fn(&Path, bool) -> String,
    pub validate_name: &'a dyn Fn(&str) -> Result<()>,
}

/// Runs the prompt flow. Returns `None` when the user cancels; nothing is
/// written by this function.
pub fn run_wizard(targets: WizardTargets) -> Result<Option<WizardOutcome>> {
    let theme = ColorfulTheme::default();
    println!(
        "Create an agent profile. Press Esc to cancel; nothing is saved until the last step.\n"
    );

    // 1. Coding agent.
    let Some(agent_index) = select(
        &theme,
        "Coding agent",
        &["Claude Code", "Codex", "Other"],
        0,
    )?
    else {
        return Ok(None);
    };
    let agent = match agent_index {
        0 => CodingAgent::ClaudeCode,
        1 => CodingAgent::Codex,
        _ => CodingAgent::Other(text(&theme, "Command that starts the agent", None, false)?),
    };

    // 2. Credentials.
    let credentials = match select(
        &theme,
        "Credentials the agent may use",
        &[
            "None",
            "Secrets from a local file",
            "Secrets from a Stashbase environment",
        ],
        0,
    )? {
        None => return Ok(None),
        Some(0) => CredentialSource::None,
        Some(1) => {
            let path = loop {
                let path = text(&theme, "Path to the secrets file", Some(".env"), false)?;
                if Path::new(&path).is_file() {
                    break path;
                }
                println!("{path} was not found. Create it first, or enter another path.");
            };
            CredentialSource::File {
                path,
                secrets: prompt_secrets(&theme)?,
            }
        }
        Some(_) => {
            let project = text(&theme, "Stashbase project", None, false)?;
            let environment = text(&theme, "Environment", Some("development"), false)?;
            CredentialSource::Environment {
                project,
                environment,
                secrets: prompt_secrets(&theme)?,
            }
        }
    };
    let credentials = match credentials {
        CredentialSource::File { secrets, .. } | CredentialSource::Environment { secrets, .. }
            if secrets.is_empty() =>
        {
            println!("No secrets added; the profile will not load any credentials.");
            CredentialSource::None
        }
        other => other,
    };

    // 3. Network.
    let mut partial = WizardAnswers {
        agent: agent.clone(),
        credentials,
        extra_hosts: Vec::new(),
        protect_files: true,
        docker: false,
    };
    let allowed = partial.egress_hosts();
    println!(
        "The agent can reach only: {}. Everything else is blocked.",
        if allowed.is_empty() {
            "nothing".to_string()
        } else {
            allowed.join(", ")
        }
    );
    let extra = text(
        &theme,
        "Other hosts to allow (comma-separated, empty for none)",
        None,
        true,
    )?;
    partial.extra_hosts = extra
        .split([',', ' '])
        .map(str::trim)
        .filter(|host| !host.is_empty())
        .map(str::to_string)
        .collect();

    // 4. Runtime.
    let Some(backend) = select(
        &theme,
        "Sandbox",
        &[
            "Native (built-in OS sandbox)",
            "Docker (requires Docker running)",
        ],
        0,
    )?
    else {
        return Ok(None);
    };
    partial.docker = backend == 1;

    // 5. Files. The same paths are written either way, so the profile stays
    // protected if a run switches it to native with `--docker-sandbox=false`.
    let files_prompt = if partial.docker {
        "Your home folder isn't mounted in Docker. Also block .env (read) and .git (write) in this repo?"
    } else {
        "Block the agent from ~/.ssh, ~/.aws, .env (read) and .git (write)?"
    };
    let Some(protect) = confirm(&theme, files_prompt, true)? else {
        return Ok(None);
    };
    partial.protect_files = protect;

    // 6. Name and location, without silently replacing an existing profile.
    let global = if targets.preset_global {
        true
    } else {
        match select(
            &theme,
            "Save the profile",
            &[
                "In this repository (.stashbase/agents)",
                "Globally (usable from any directory)",
            ],
            0,
        )? {
            Some(index) => index == 1,
            None => return Ok(None),
        }
    };
    let mut default_name = agent.command().to_string();
    let (profile, path, overwrite) = loop {
        let name = text(&theme, "Profile name", Some(&default_name), false)?;
        if let Err(error) = (targets.validate_name)(&name) {
            println!("{error}");
            continue;
        }
        let path = (targets.resolve)(&name, global)?;
        if !path.exists() {
            break (name, path, false);
        }
        if targets.force {
            break (name, path, true);
        }
        let shown = (targets.display)(&path, global);
        match select(
            &theme,
            &format!("{shown} already exists"),
            &["Choose a different name", "Overwrite it", "Cancel"],
            0,
        )? {
            Some(0) => default_name = format!("{name}-2"),
            Some(1) if confirm(&theme, &format!("Replace {shown}?"), false)? == Some(true) => {
                break (name, path, true)
            }
            Some(1) => {}
            _ => return Ok(None),
        }
    };

    // 7. Review and validate before asking to save.
    let contents = render_profile(&partial)?;
    print_summary(&partial, &(targets.display)(&path, global));
    if confirm(&theme, "Save this profile?", true)? != Some(true) {
        return Ok(None);
    }

    Ok(Some(WizardOutcome {
        profile,
        path,
        global,
        contents,
        agent_command: agent.command().to_string(),
        overwrite,
    }))
}

fn prompt_secrets(theme: &ColorfulTheme) -> Result<Vec<SecretGrant>> {
    println!(
        "Each secret is sent only to the host you name, read-only (GET/HEAD) unless you allow writes.\n\
         The agent sees it under the same name; to rename it, add env = \"NEW_NAME\" in the profile."
    );
    let mut secrets: Vec<SecretGrant> = Vec::new();
    loop {
        let prompt = if secrets.is_empty() {
            "Secret name"
        } else {
            "Another secret name (empty to finish)"
        };
        let name = text(theme, prompt, None, !secrets.is_empty())?;
        if name.is_empty() {
            return Ok(secrets);
        }
        if secrets.iter().any(|secret| secret.name == name) {
            println!("{name} is already added.");
            continue;
        }
        let env = name.clone();
        let host = text(
            theme,
            "Host this secret is for (e.g. api.github.com)",
            None,
            false,
        )?;
        let allow_writes =
            confirm(theme, "Also allow writes (POST/PUT/PATCH/DELETE)?", false)?.unwrap_or(false);
        secrets.push(SecretGrant {
            name,
            env,
            host,
            allow_writes,
        });
    }
}

fn print_summary(answers: &WizardAnswers, path: &str) {
    println!("\nProfile: {path}");
    println!("  Agent:       {}", answers.agent.label());
    match &answers.credentials {
        CredentialSource::None => println!("  Credentials: none"),
        CredentialSource::File { path, .. } => println!("  Credentials: from {path}"),
        CredentialSource::Environment {
            project,
            environment,
            ..
        } => {
            println!("  Credentials: Stashbase {project}/{environment}")
        }
    }
    for secret in answers.credentials.secrets() {
        let access = if secret.allow_writes {
            "read + write"
        } else {
            "read-only"
        };
        println!("    {} -> {} ({access})", secret.name, secret.host);
    }
    let hosts = answers.egress_hosts();
    println!(
        "  Network:     {}",
        if hosts.is_empty() {
            "all blocked".to_string()
        } else {
            hosts.join(", ")
        }
    );
    println!(
        "  Files:       {}",
        if answers.protect_files && answers.docker {
            ".env, .git protected (home folder not mounted)"
        } else if answers.protect_files {
            "~/.ssh, ~/.aws, .env, .git protected"
        } else if answers.docker {
            "home folder not mounted; repo files unprotected"
        } else {
            "no extra protection"
        }
    );
    println!("  MCP, hooks:  off");
    println!(
        "  Sandbox:     {}\n",
        if answers.docker { "Docker" } else { "native" }
    );
}

fn select(
    theme: &ColorfulTheme,
    prompt: &str,
    items: &[&str],
    default: usize,
) -> Result<Option<usize>> {
    Select::with_theme(theme)
        .with_prompt(prompt)
        .items(items)
        .default(default)
        .interact_opt()
        .context("Could not read the selection.")
}

fn confirm(theme: &ColorfulTheme, prompt: &str, default: bool) -> Result<Option<bool>> {
    Confirm::with_theme(theme)
        .with_prompt(prompt)
        .default(default)
        .interact_opt()
        .context("Could not read the answer.")
}

fn text(
    theme: &ColorfulTheme,
    prompt: &str,
    default: Option<&str>,
    allow_empty: bool,
) -> Result<String> {
    let mut input = Input::<String>::with_theme(theme)
        .with_prompt(prompt)
        .allow_empty(allow_empty);
    if let Some(default) = default {
        input = input.default(default.to_string());
    }
    let value = input.interact_text().context("Could not read the input.")?;
    let value = value.trim().to_string();
    if value.is_empty() && !allow_empty {
        bail!("{prompt} is required.");
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answers(credentials: CredentialSource) -> WizardAnswers {
        WizardAnswers {
            agent: CodingAgent::ClaudeCode,
            credentials,
            extra_hosts: vec![],
            protect_files: true,
            docker: false,
        }
    }

    fn grant(allow_writes: bool) -> SecretGrant {
        SecretGrant {
            name: "GITHUB_TOKEN".into(),
            env: "GH_TOKEN".into(),
            host: "api.github.com".into(),
            allow_writes,
        }
    }

    fn parse(toml: &str) -> AgentProfile {
        toml::from_str(toml).unwrap()
    }

    #[test]
    fn no_credentials_profile_is_restrictive_and_valid() {
        let profile = parse(&render_profile(&answers(CredentialSource::None)).unwrap());
        assert_eq!(
            profile.egress_hosts.unwrap(),
            vec!["api.anthropic.com", "platform.claude.com"]
        );
        assert!(profile.file.is_none());
        assert!(profile.secrets.bindings.is_empty());
        assert!(profile.mcp_servers.is_empty());
        assert!(profile.allow_hooks.is_empty());
        assert_eq!(profile.filesystem.deny_read, PROTECTED_READ);
    }

    #[test]
    fn environment_secret_is_read_only_by_default() {
        let toml = render_profile(&answers(CredentialSource::Environment {
            project: "app".into(),
            environment: "dev".into(),
            secrets: vec![grant(false)],
        }))
        .unwrap();
        let profile = parse(&toml);
        assert_eq!(profile.secrets.project.as_deref(), Some("app"));
        let binding = &profile.secrets.bindings["GITHUB_TOKEN"];
        assert_eq!(binding.env.as_deref(), Some("GH_TOKEN"));
        assert_eq!(binding.rules[0].methods, vec!["GET", "HEAD"]);
        assert_eq!(binding.rules[0].hosts, vec!["api.github.com"]);
        assert!(profile
            .egress_hosts
            .unwrap()
            .contains(&"api.github.com".to_string()));
    }

    #[test]
    fn file_secret_with_writes_and_docker() {
        // Validation requires the secrets file to exist; don't rely on the checkout.
        let file =
            std::env::temp_dir().join(format!("stashbase-init-wizard-{}.env", std::process::id()));
        std::fs::write(&file, "GITHUB_TOKEN=fake\n").unwrap();
        let file = file.to_string_lossy().into_owned();
        let mut a = answers(CredentialSource::File {
            path: file.clone(),
            secrets: vec![grant(true)],
        });
        a.docker = true;
        a.agent = CodingAgent::Codex;
        a.extra_hosts = vec!["registry.npmjs.org".into(), "chatgpt.com".into()];
        let profile = parse(&render_profile(&a).unwrap());
        assert_eq!(profile.file.as_deref(), Some(file.as_str()));
        let _ = std::fs::remove_file(&file);
        assert!(profile.secrets.bindings["GITHUB_TOKEN"].rules[0]
            .methods
            .contains(&"DELETE".to_string()));
        assert_eq!(
            profile.sandbox.backend,
            crate::models::agent::SandboxBackend::Docker
        );
        assert_eq!(
            profile.egress_hosts.unwrap(),
            vec![
                "chatgpt.com",
                "api.openai.com",
                "auth.openai.com",
                "api.github.com",
                "registry.npmjs.org"
            ]
        );
    }

    #[test]
    fn output_never_contains_more_than_names() {
        let toml = render_profile(&answers(CredentialSource::Environment {
            project: "app".into(),
            environment: "dev".into(),
            secrets: vec![grant(false)],
        }))
        .unwrap();
        assert!(!toml.contains("value"));
    }

    #[test]
    fn optional_examples_are_commented_and_listed() {
        let toml = render_profile(&answers(CredentialSource::None)).unwrap();
        assert!(toml.contains("# [mcp_servers.example]"));
        assert!(toml.contains("# [secrets.SECRET_NAME]"));
        assert!(toml.contains("# allow_hooks"));
        assert!(!toml.contains("# [filesystem]"));
        let profile = parse(&toml);
        assert!(profile.mcp_servers.is_empty() && profile.config_env.is_empty());
    }

    #[test]
    fn unprotected_other_agent_has_no_hosts() {
        let mut a = answers(CredentialSource::None);
        a.agent = CodingAgent::Other("aider".into());
        a.protect_files = false;
        let profile = parse(&render_profile(&a).unwrap());
        assert!(profile.egress_hosts.unwrap().is_empty());
        assert!(profile.filesystem.deny_read.is_empty());
    }
}
