use std::collections::HashMap;

use anyhow::Result;

use log::debug;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::select;

pub async fn run_command(
    command: &str,
    args: Vec<&str>,
    env_vars: HashMap<String, String>,
) -> Result<()> {
    // Define the command you want to run
    let mut child = Command::new(command)
        .args(args)
        .envs(env_vars)
        .env("FORCE_COLOR", "true")
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;

    // Create a Tokio stream for stdout
    let stdout = child.stdout.take().expect("Failed to get stdout");
    let mut stdout_stream = BufReader::new(stdout).lines();

    // Create a Tokio stream for stderr
    let stderr = child.stderr.take().expect("Failed to get stderr");
    let mut stderr_stream = BufReader::new(stderr).lines();

    // Use a loop to select between stdout and stderr streams
    loop {
        select! {
            Ok(line) = stdout_stream.next_line() => match line {
                Some(line) => println!("{}", line),
                None => break,
            },
            Ok(line) = stderr_stream.next_line() => match line {
                Some(line) => eprintln!("{}", line),
                None => break,
            },
            else => break, // Exit the loop when both streams are exhausted
        }
    }

    // Wait for the child process to finish
    let status = child.wait().await;
    // child.kill().await?;
    debug!("Command exited with: {:?}", status);

    Ok(())
}
