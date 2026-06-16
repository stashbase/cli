use std::collections::HashMap;
use std::env;

use anyhow::Result;
use duct::cmd;

// use log::debug;
use std::io::prelude::*;
use std::io::BufReader;

// for now stdout to stderr - working great
pub async fn run_command(
    command: &str,
    args: Vec<String>,
    env_vars: HashMap<String, String>,
) -> Result<()> {
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

    let reader = cmd.stdout_to_stderr().reader()?;
    let mut lines = BufReader::new(reader).lines();

    loop {
        if let Some(line) = lines.next() {
            if let Ok(line) = line {
                println!("{}", line);
            } else {
                break;
            }
        } else {
            break;
        }
    }

    Ok(())
}
