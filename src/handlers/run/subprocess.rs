use std::collections::HashMap;
use std::env;
use std::process::ExitStatus;

#[cfg(not(target_os = "macos"))]
use anyhow::bail;
use anyhow::Result;
use duct::{cmd, Expression};
use thiserror::Error;

// use log::debug;
use std::io::prelude::*;
use std::io::BufReader;

const RESTRICTED_CHILD_ENV_REMOVALS: &[&str] = &["STASHBASE_API_KEY"];
// These settings can make a child select another proxy or skip the broker for
// selected hosts. The broker re-adds its own HTTP(S)_PROXY values afterwards.
const BROKER_CHILD_ENV_REMOVALS: &[&str] = &[
    "NO_PROXY",
    "no_proxy",
    "ALL_PROXY",
    "all_proxy",
    "NPM_CONFIG_NOPROXY",
    "npm_config_noproxy",
    "NPM_CONFIG_PROXY",
    "npm_config_proxy",
    "NPM_CONFIG_HTTPS_PROXY",
    "npm_config_https_proxy",
];

// for now stdout to stderr - working great
#[derive(Debug, Error)]
#[error("command exited with status {status}")]
pub struct CommandFailed {
    pub status: ExitStatus,
}

impl CommandFailed {
    pub fn exit_code(&self) -> i32 {
        self.status.code().unwrap_or(1)
    }
}

pub async fn run_command(
    command: &str,
    args: Vec<String>,
    env_vars: HashMap<String, String>,
    sandbox: bool,
    broker_mode: bool,
    restrict_stashbase_credentials: bool,
) -> Result<ExitStatus> {
    let current_dir = env::current_dir()?;
    let (program, launcher_args) = sandbox_command(command, sandbox, &env_vars)?;
    let cmd: Expression = cmd(program, launcher_args)
        .before_spawn(move |cmd| {
            if restrict_stashbase_credentials {
                // Do not inherit the developer's Stashbase API key into a
                // restricted agent child. Explicit profile placeholders are
                // added below and remain supported.
                for name in RESTRICTED_CHILD_ENV_REMOVALS {
                    cmd.env_remove(name);
                }
            }
            if broker_mode {
                // Clear parent and tool-specific proxy overrides before applying
                // the broker's explicit proxy environment below. `NO_PROXY` is
                // then set to an empty value by Broker::child_env().
                for name in BROKER_CHILD_ENV_REMOVALS {
                    cmd.env_remove(name);
                }
            }
            for arg in args.iter() {
                cmd.arg(arg);
            }
            for (env, value) in env_vars.iter() {
                cmd.env(env, value);
            }
            Ok(())
        })
        .env("FORCE_COLOR", "true")
        .dir(current_dir);
    // .full_env(env_vars);
    // .env("--color", "always");

    // `unchecked` lets us drain output before returning the child's exact status.
    let mut reader = cmd.stdout_to_stderr().unchecked().reader()?;
    {
        let mut lines = BufReader::new(&mut reader).lines();
        loop {
            match lines.next() {
                Some(line) => println!("{}", line?),
                None => break,
            }
        }
    }

    let output = reader
        .try_wait()?
        .expect("reader reached EOF after child exit");
    Ok(output.status)
}

fn sandbox_command(
    command: &str,
    sandbox: bool,
    env_vars: &HashMap<String, String>,
) -> Result<(String, Vec<String>)> {
    if !sandbox {
        return Ok((command.to_owned(), Vec::new()));
    }

    #[cfg(target_os = "macos")]
    {
        let broker_port = env_vars
            .get("HTTPS_PROXY")
            .and_then(|url| url.rsplit_once(':').map(|(_, port)| port))
            .filter(|port| port.parse::<u16>().is_ok())
            .ok_or_else(|| anyhow::anyhow!("--sandbox requires a localhost HTTPS_PROXY"))?;

        // The agent can only make network connections to this command's loopback broker.
        // `allow default` keeps language runtimes usable; direct outbound and inbound
        // sockets are then denied, with the broker's exact port restored as the exception.
        let profile = format!(
            r#"
            (version 1)
            (allow default)
            (deny network-inbound)
            (deny network-outbound)
            (allow network-outbound (remote ip "localhost:{broker_port}"))
        "#
        );
        return Ok((
            "/usr/bin/sandbox-exec".to_owned(),
            vec!["-p".to_owned(), profile, command.to_owned()],
        ));
    }

    #[cfg(not(target_os = "macos"))]
    {
        bail!("--sandbox is currently supported only on macOS")
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::{run_command, sandbox_command};
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};

    fn environment_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[tokio::test]
    async fn returns_the_child_exit_status() {
        let status = run_command(
            "sh",
            vec!["-c".to_owned(), "exit 7".to_owned()],
            HashMap::new(),
            false,
            false,
            false,
        )
        .await
        .unwrap();

        assert_eq!(status.code(), Some(7));
    }

    #[tokio::test]
    async fn restricted_child_does_not_inherit_stashbase_api_key() {
        let _guard = environment_lock().lock().unwrap();
        let previous = std::env::var_os("STASHBASE_API_KEY");
        std::env::set_var("STASHBASE_API_KEY", "parent-api-key");

        let status = run_command(
            "sh",
            vec!["-c".to_owned(), "test -z \"$STASHBASE_API_KEY\"".to_owned()],
            HashMap::new(),
            false,
            false,
            true,
        )
        .await
        .unwrap();

        match previous {
            Some(value) => std::env::set_var("STASHBASE_API_KEY", value),
            None => std::env::remove_var("STASHBASE_API_KEY"),
        }
        assert!(status.success());
    }

    #[tokio::test]
    async fn broker_child_clears_proxy_bypass_overrides() {
        let _guard = environment_lock().lock().unwrap();
        let saved = [
            ("NO_PROXY", std::env::var_os("NO_PROXY")),
            ("ALL_PROXY", std::env::var_os("ALL_PROXY")),
            ("npm_config_proxy", std::env::var_os("npm_config_proxy")),
        ];
        std::env::set_var("NO_PROXY", "api.example.com");
        std::env::set_var("ALL_PROXY", "http://other-proxy.invalid");
        std::env::set_var("npm_config_proxy", "http://other-proxy.invalid");

        let status = run_command(
            "sh",
            vec![
                "-c".to_owned(),
                "test -z \"$NO_PROXY\" && test -z \"$ALL_PROXY\" && test -z \"$npm_config_proxy\""
                    .to_owned(),
            ],
            HashMap::from([
                ("HTTP_PROXY".to_owned(), "http://127.0.0.1:9999".to_owned()),
                ("HTTPS_PROXY".to_owned(), "http://127.0.0.1:9999".to_owned()),
                ("NO_PROXY".to_owned(), String::new()),
                ("no_proxy".to_owned(), String::new()),
            ]),
            false,
            true,
            false,
        )
        .await
        .unwrap();

        for (name, value) in saved {
            match value {
                Some(value) => std::env::set_var(name, value),
                None => std::env::remove_var(name),
            }
        }
        assert!(status.success());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_sandbox_allows_only_the_configured_broker_port() {
        use std::{net::TcpListener, process::Command, thread};

        let allowed_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let allowed_port = allowed_listener.local_addr().unwrap().port();
        let accepted = thread::spawn(move || allowed_listener.accept().is_ok());
        let env_vars = HashMap::from([(
            "HTTPS_PROXY".to_owned(),
            format!("http://127.0.0.1:{allowed_port}"),
        )]);
        let (program, launcher_args) = sandbox_command("/usr/bin/nc", true, &env_vars).unwrap();
        let allowed_port = allowed_port.to_string();

        let allowed = Command::new(&program)
            .args(&launcher_args)
            .args(["-z", "127.0.0.1", &allowed_port])
            .status()
            .unwrap();
        assert!(allowed.success());
        assert!(accepted.join().unwrap());

        let denied_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let denied_port = denied_listener.local_addr().unwrap().port().to_string();
        let denied = Command::new(program)
            .args(launcher_args)
            .args(["-z", "127.0.0.1", &denied_port])
            .status()
            .unwrap();
        assert!(!denied.success());
    }
}
