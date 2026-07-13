use std::collections::HashMap;
use std::env;
use std::process::ExitStatus;

use anyhow::Result;
use duct::cmd;
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
) -> Result<ExitStatus> {
    let current_dir = env::current_dir()?;
    let cmd = cmd!(command)
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
        )
        .await
        .unwrap();

        assert_eq!(status.code(), Some(7));
    }
}
