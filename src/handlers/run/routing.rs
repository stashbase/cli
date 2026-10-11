//! Which destinations the local relay sends through the remote Agent Proxy.
//!
//! In `credential` mode only hosts the control plane lists (plus any host
//! bound to a personal credential) go remote; everything else is forwarded by
//! the local relay itself. The table is built from the session response and
//! is never read from profiles, config, or the environment.

use std::{collections::HashSet, sync::Arc};

use anyhow::{bail, Result};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};

use crate::api::remote_proxy::{RemoteBinding, RemoteProxySession};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
#[value(rename_all = "snake_case")]
pub enum RemoteMode {
    /// Only requests that need a Stashbase credential use the remote proxy.
    Credential,
    /// All agent traffic uses the remote proxy.
    Full,
}

impl RemoteMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Credential => "credential",
            Self::Full => "full",
        }
    }
}

/// Exact hosts and explicit `*.suffix` wildcards. Matching is by whole label
/// only, never by substring, and `*.example.com` does not match `example.com`
/// (the same rule as `policy::host_matches`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RouteTable {
    exact: HashSet<String>,
    suffixes: HashSet<String>,
}

impl RouteTable {
    pub fn parse<S: AsRef<str>>(hosts: &[S]) -> Result<Self> {
        let mut table = Self::default();
        for host in hosts {
            table.insert(host.as_ref())?;
        }
        Ok(table)
    }

    /// Adds a host that must always be remote, such as a personal credential's.
    pub fn insert(&mut self, entry: &str) -> Result<()> {
        let entry = entry.trim().trim_end_matches('.').to_ascii_lowercase();
        match entry.strip_prefix("*.") {
            Some(suffix) => {
                validate_hostname(&entry, suffix)?;
                if !suffix.contains('.') {
                    bail!("route host `{entry}` is too broad; wildcards need at least two labels");
                }
                self.suffixes.insert(suffix.to_owned());
            }
            None => {
                validate_hostname(&entry, &entry)?;
                self.exact.insert(entry);
            }
        }
        Ok(())
    }

    pub fn is_empty(&self) -> bool {
        self.exact.is_empty() && self.suffixes.is_empty()
    }

    pub fn len(&self) -> usize {
        self.exact.len() + self.suffixes.len()
    }

    /// `host` must not include a port.
    pub fn matches(&self, host: &str) -> bool {
        let host = host.trim_end_matches('.').to_ascii_lowercase();
        if self.exact.contains(&host) {
            return true;
        }
        // Walk the parent domains: `a.b.example.com` -> `b.example.com` -> ...
        let mut rest = host.as_str();
        while let Some((_, parent)) = rest.split_once('.') {
            if self.suffixes.contains(parent) {
                return true;
            }
            rest = parent;
        }
        false
    }
}

/// Resolves the routing a control-plane session response asks for.
///
/// Hosts of every credential binding (project secrets and personal
/// credentials) and `extra_remote_hosts` are always remote, whatever the
/// control plane's list says. A binding's rules may name additional hosts, so
/// those count too. Adding hosts can only send more traffic remote.
pub fn resolve_session_routing(
    session: &RemoteProxySession,
    bindings: &[RemoteBinding],
    extra_remote_hosts: &[String],
) -> Result<RemoteRouting> {
    let binding_hosts = bindings
        .iter()
        .flat_map(|binding| {
            binding
                .hosts
                .iter()
                .chain(binding.rules.iter().flat_map(|rule| rule.hosts.iter()))
        })
        .chain(extra_remote_hosts.iter())
        .map(String::as_str);
    RemoteRouting::resolve(
        session.routing_mode,
        session.route_hosts.as_deref(),
        binding_hosts,
        !bindings.is_empty(),
    )
}

/// The effective routing for one remote session. Cheap to clone; the relay
/// swaps it atomically when a replacement session is issued.
#[derive(Debug, Clone)]
pub struct RemoteRouting {
    pub mode: RemoteMode,
    pub routes: Arc<RouteTable>,
}

impl RemoteRouting {
    /// Everything remote, as before credential routing existed.
    pub fn full() -> Self {
        Self {
            mode: RemoteMode::Full,
            routes: Arc::new(RouteTable::default()),
        }
    }

    /// Resolves what the control plane returned. Never goes direct on missing
    /// information: a control plane without credential routing is treated as
    /// `full`, and a credential-mode session with an unusable host list fails.
    ///
    /// `binding_hosts` are hosts bound to credentials; they are always remote
    /// because the CLI never injects a credential value locally. A binding
    /// that covers every host (`*`) means all traffic is credentialed, so the
    /// session routes everything remote.
    pub fn resolve<'a>(
        mode: Option<RemoteMode>,
        route_hosts: Option<&[String]>,
        binding_hosts: impl IntoIterator<Item = &'a str>,
        has_bindings: bool,
    ) -> Result<Self> {
        match mode {
            None | Some(RemoteMode::Full) => Ok(Self::full()),
            Some(RemoteMode::Credential) => {
                let Some(route_hosts) = route_hosts else {
                    bail!("Agent Proxy selected credential routing but sent no route_hosts");
                };
                let mut routes = RouteTable::parse(route_hosts)?;
                for host in binding_hosts {
                    if host.trim() == "*" {
                        return Ok(Self::full());
                    }
                    routes.insert(host)?;
                }
                if has_bindings && routes.is_empty() {
                    bail!("Agent Proxy selected credential routing with an empty route_hosts list for a session that has credential bindings");
                }
                Ok(Self {
                    mode: RemoteMode::Credential,
                    routes: Arc::new(routes),
                })
            }
        }
    }

    /// True when a request to `host` must use the remote proxy.
    pub fn is_remote(&self, host: &str) -> bool {
        match self.mode {
            RemoteMode::Full => true,
            RemoteMode::Credential => self.routes.matches(host),
        }
    }
}

fn validate_hostname(entry: &str, name: &str) -> Result<()> {
    let valid = !name.is_empty()
        && name.is_ascii()
        && !name.starts_with('.')
        && !name.contains("..")
        && name.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'.' || byte == b'_'
        });
    if !valid {
        bail!("invalid route host `{entry}`");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(hosts: &[&str]) -> RouteTable {
        RouteTable::parse(hosts).unwrap()
    }

    #[test]
    fn exact_hosts_match_whole_host_only() {
        let routes = table(&["api.github.com"]);
        assert!(routes.matches("api.github.com"));
        assert!(routes.matches("API.GitHub.com."));
        assert!(!routes.matches("github.com"));
        assert!(!routes.matches("x.api.github.com"));
    }

    #[test]
    fn never_matches_by_substring() {
        let routes = table(&["github.com", "*.example.com"]);
        assert!(!routes.matches("evil-github.com"));
        assert!(!routes.matches("github.com.evil.com"));
        assert!(!routes.matches("notexample.com"));
        assert!(!routes.matches("example.com.evil.com"));
    }

    #[test]
    fn wildcard_matches_subdomains_but_not_the_apex() {
        let routes = table(&["*.example.com"]);
        assert!(routes.matches("a.example.com"));
        assert!(routes.matches("a.b.example.com"));
        assert!(!routes.matches("example.com"));
    }

    #[test]
    fn agrees_with_policy_host_matches() {
        for (allowed, host) in [
            ("*.example.com", "a.example.com"),
            ("*.example.com", "example.com"),
            ("*.example.com", "a.b.example.com"),
            ("example.com", "example.com"),
            ("example.com", "a.example.com"),
        ] {
            assert_eq!(
                table(&[allowed]).matches(host),
                crate::handlers::agent::policy::host_matches(allowed, host),
                "{allowed} vs {host}"
            );
        }
    }

    #[test]
    fn rejects_unsafe_entries() {
        for bad in [
            "*",
            "*.com",
            "a*b.com",
            "*.*.com",
            "",
            "host:443",
            "https://a.com",
            "a.com/path",
            "user@a.com",
            " a b.com",
            ".a.com",
            "a..com",
            "bücher.de",
        ] {
            assert!(
                RouteTable::parse(&[bad]).is_err(),
                "{bad:?} should be rejected"
            );
        }
    }

    #[test]
    fn insert_adds_always_remote_hosts() {
        let mut routes = table(&["a.com"]);
        routes.insert("Api.Personal.dev").unwrap();
        assert!(routes.matches("api.personal.dev"));
        assert_eq!(routes.len(), 2);
    }

    #[test]
    fn legacy_control_plane_means_full_routing() {
        let routing = RemoteRouting::resolve(None, None, [], true).unwrap();
        assert_eq!(routing.mode, RemoteMode::Full);
        assert!(routing.is_remote("anything.example"));
    }

    #[test]
    fn credential_routing_sends_only_listed_hosts_remote() {
        let hosts = vec!["api.github.com".to_owned()];
        let routing =
            RemoteRouting::resolve(Some(RemoteMode::Credential), Some(&hosts), [], true).unwrap();
        assert!(routing.is_remote("api.github.com"));
        assert!(!routing.is_remote("api.anthropic.com"));
    }

    #[test]
    fn personal_credential_hosts_are_always_remote() {
        let hosts = vec!["api.github.com".to_owned()];
        let routing = RemoteRouting::resolve(
            Some(RemoteMode::Credential),
            Some(&hosts),
            ["mcp.linear.app"],
            true,
        )
        .unwrap();
        assert!(routing.is_remote("mcp.linear.app"));
    }

    #[test]
    fn secret_and_personal_binding_hosts_are_remote_even_if_the_backend_list_omits_them() {
        use crate::api::remote_proxy::RemoteBindingSource;
        let session: RemoteProxySession = serde_json::from_value(serde_json::json!({
            "session_id": "s", "session_token": "t", "expires_at": "2026-01-01T00:00:00Z",
            "proxy_url": "/proxy", "protocol": "http/1.1-custom",
            "routing_mode": "credential", "route_hosts": ["other.example"]
        }))
        .unwrap();
        let binding = |source, host: &str| RemoteBinding {
            name: "TOKEN".to_owned(),
            source,
            source_name: "TOKEN".to_owned(),
            hosts: vec![host.to_owned()],
            rules: Vec::new(),
            header: "authorization".to_owned(),
            placeholder: "${STASHBASE_TOKEN}".to_owned(),
            value_template: "Bearer {value}".to_owned(),
        };
        let bindings = [
            binding(RemoteBindingSource::Secret, "api.github.com"),
            binding(RemoteBindingSource::PersonalCredential, "mcp.linear.app"),
        ];

        let routing = resolve_session_routing(&session, &bindings, &[]).unwrap();

        assert!(routing.is_remote("api.github.com"));
        assert!(routing.is_remote("mcp.linear.app"));
        assert!(routing.is_remote("other.example"));
        assert!(!routing.is_remote("api.anthropic.com"));
    }

    #[test]
    fn a_binding_covering_every_host_routes_everything_remote() {
        let hosts = vec!["api.github.com".to_owned()];
        let routing =
            RemoteRouting::resolve(Some(RemoteMode::Credential), Some(&hosts), ["*"], true)
                .unwrap();
        assert!(routing.is_remote("anything.example"));
    }

    #[test]
    fn credential_routing_without_a_usable_list_fails_closed() {
        assert!(RemoteRouting::resolve(Some(RemoteMode::Credential), None, [], false).is_err());
        assert!(RemoteRouting::resolve(Some(RemoteMode::Credential), Some(&[]), [], true).is_err());
        let invalid = vec!["*".to_owned()];
        assert!(
            RemoteRouting::resolve(Some(RemoteMode::Credential), Some(&invalid), [], true).is_err()
        );
    }

    #[test]
    fn credential_routing_without_bindings_may_have_an_empty_list() {
        let routing =
            RemoteRouting::resolve(Some(RemoteMode::Credential), Some(&[]), [], false).unwrap();
        assert!(!routing.is_remote("a.com"));
    }

    #[test]
    fn empty_table_matches_nothing() {
        assert!(table(&[]).is_empty());
        assert!(!table(&[]).matches("a.com"));
    }
}
