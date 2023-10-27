use std::collections::HashMap;
use std::env;

use anyhow::Result;
use duct::cmd;

// use log::debug;
use std::io::prelude::*;
use std::io::BufReader;
// use tokio::io::AsyncBufReadExt;
// use tokio::process::Command;
// use tokio::select;

// for now stdout to stderr - working great
pub async fn run_command(
    command: &str,
    args: Vec<String>,
    env_vars: HashMap<String, String>,
) -> Result<()> {
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
        .dir(env::current_dir().unwrap());
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

    // // Define the command you want to run
    // let mut child = Command::new(command)
    //     .args(args)
    //     .envs(env_vars)
    //     .env("FORCE_COLOR", "true")
    //     .stdin(std::process::Stdio::inherit())
    //     .stdout(std::process::Stdio::piped())
    //     .stderr(std::process::Stdio::piped())
    //     .spawn()?;
    //
    // // Create a Tokio stream for stdout
    // let stdout = child.stdout.take().expect("Failed to get stdout");
    // let mut stdout_stream = BufReader::new(stdout).lines();
    //
    // // Create a Tokio stream for stderr
    // let stderr = child.stderr.take().expect("Failed to get stderr");
    // let mut stderr_stream = BufReader::new(stderr).lines();
    //
    // // Use a loop to select between stdout and stderr streams
    // loop {
    //     select! {
    //         Ok(line) = stdout_stream.next_line() => match line {
    //             Some(line) => println!("{}", line),
    //             None => break,
    //         },
    //         Ok(line) = stderr_stream.next_line() => match line {
    //             Some(line) => eprintln!("{}", line),
    //             None => break,
    //         },
    //         else => break, // Exit the loop when both streams are exhausted
    //     }
    // }
    //
    // // Wait for the child process to finish
    // let status = child.wait().await;
    // // child.kill().await?;
    // debug!("Command exited with: {:?}", status);
    //
    // Ok(())
}
