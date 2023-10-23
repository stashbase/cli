use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::select;

use std::process::Stdio;

pub async fn test_stream_1() -> Result<(), Box<dyn std::error::Error>> {
    // Define the command you want to run
    let mut child = tokio::process::Command::new("make")
        .arg("dev")
        .stdin(std::process::Stdio::piped())
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
                Some(line) => println!("stdout: {}", line),
                None => break,
            },
            Ok(line) = stderr_stream.next_line() => match line {
                Some(line) => eprintln!("stderr: {}", line),
                None => break,
            },
            else => break, // Exit the loop when both streams are exhausted
        }
    }

    // Wait for the child process to finish
    let status = child.wait().await;
    println!("Command exited with: {:?}", status);

    Ok(())
}

// #[tokio::main]
pub async fn test_stream() -> Result<(), Box<dyn std::error::Error>> {
    let mut cmd = Command::new("cat");

    // Specify that we want the command's standard output piped back to us.
    // By default, standard input/output/error will be inherited from the
    // current process (for example, this means that standard input will
    // come from the keyboard and standard output/error will go directly to
    // the terminal if this process is invoked from the command line).
    cmd.stdout(Stdio::piped());

    let mut child = cmd.spawn().expect("failed to spawn command");

    let stdout = child
        .stdout
        .take()
        .expect("child did not have a handle to stdout");

    let mut reader = BufReader::new(stdout).lines();

    // Ensure the child process is spawned in the runtime so it can
    // make progress on its own while we await for any output.
    tokio::spawn(async move {
        let status = child
            .wait()
            .await
            .expect("child process encountered an error");

        println!("child status was: {}", status);
    });

    while let Some(line) = reader.next_line().await? {
        println!("Line: {}", line);
    }

    Ok(())
}
