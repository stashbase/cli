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
) -> Result<ExitStatus> {
    let current_dir = env::current_dir()?;
    let (program, launcher_args) = sandbox_command(command, sandbox)?;
    let cmd: Expression = cmd(program, launcher_args)
        .before_spawn(move |cmd| {
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

fn sandbox_command(command: &str, sandbox: bool) -> Result<(String, Vec<String>)> {
    if !sandbox {
        return Ok((command.to_owned(), Vec::new()));
    }

    #[cfg(target_os = "macos")]
    {
        // The agent can only make network connections to the loopback broker.
        // `allow default` keeps language runtimes usable; direct outbound and inbound
        // sockets are then denied, with localhost restored as the narrow exception.
        let profile = r#"
            (version 1)
            (allow default)
            (deny network-inbound)
            (deny network-outbound)
            (allow network-outbound (remote ip "localhost:*"))
        "#;
        return Ok((
            "/usr/bin/sandbox-exec".to_owned(),
            vec!["-p".to_owned(), profile.to_owned(), command.to_owned()],
        ));
    }

    #[cfg(not(target_os = "macos"))]
    {
        bail!("--sandbox is currently supported only on macOS")
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::run_command;
    use std::collections::HashMap;

    #[tokio::test]
    async fn returns_the_child_exit_status() {
        let status = run_command(
            "sh",
            vec!["-c".to_owned(), "exit 7".to_owned()],
            HashMap::new(),
            false,
        )
        .await
        .unwrap();

        assert_eq!(status.code(), Some(7));
    }
}
