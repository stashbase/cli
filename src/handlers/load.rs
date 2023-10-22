use anyhow::{bail, Result};
use log::debug;
use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::process::{Command, Stdio};

use crate::{
    api::{environments, secrets},
    models::{api_client::GetRequestApiResponse, environments::Environment, secrets::Secret},
    utils::{spinner::request_spinner, validation::validate_project_environment},
};

pub async fn handle_load_environment(
    token: String,
    project: String,
    environment: String,
) -> Result<()> {
    let input_valid = validate_project_environment(&project, &environment, true);

    if let Err(err) = input_valid {
        bail!(err);
    }

    // OK
    debug!("loading env...");

    // let working_dir = "/home/radim/code/env-vault/env-vault-api";

    let secrets = vec![Secret {
        key: "JWT_SECRET".to_string(),
        value: "234234".to_string(),
        description: None,
    }];

    let env_vars = create_env_vars(secrets);

    // testing
    // Create a Command to run the 'npm run dev' script
    let mut child = Command::new("npm")
        .arg("run")
        .arg("dev")
        // .current_dir(working_dir) // remove
        .env("FORCE_COLOR", "true")
        .envs(&env_vars)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        //.stdout(Stdio::inherit()) // This preserves colored output
        //.stderr(Stdio::inherit()) // This preserves colored error output
        .spawn()
        .expect("Failed to start npm run dev");

    // Create a buffer to read the child process's output
    let mut buffer = Vec::new();

    // Read and display the child process's output
    loop {
        match child.stdout.as_mut().unwrap().read(&mut buffer) {
            Ok(0) => break, // End of output
            Ok(n) => {
                io::stdout()
                    .write_all(&buffer[0..n])
                    .expect("Failed to write to stdout");
                io::stdout().flush().expect("Failed to flush stdout");
            }
            Err(e) => {
                eprintln!("Error reading stdout: {}", e);
                break;
            }
        }
    }

    // Wait for the child process to finish
    let status = child.wait().expect("Failed to wait for child process");
    debug!("Child process exited with: {}", status);

    // // for (key, value) in env_vars {
    // //     child.env(key, value);
    // // }
    // //
    // // Obtain handles to the child process's standard input, output, and error streams
    // let mut stdin = child.stdin.take().expect("Failed to get stdin");
    // let mut stdout = child.stdout.take().expect("Failed to get stdout");
    // let stderr = child.stderr.take().expect("Failed to get stderr");
    //
    // // You can interact with the child process here, for example, reading from stdout
    // let mut buffer = [0; 1024];
    // thread::spawn(move || {
    //     loop {
    //         match stdout.read(&mut buffer) {
    //             Ok(0) => break, // End of stream
    //             Ok(n) => {
    //                 print!("{}", String::from_utf8_lossy(&buffer[0..n]));
    //             }
    //             Err(e) => {
    //                 eprintln!("Error reading stdout: {}", e);
    //                 break;
    //             }
    //         }
    //     }
    // });
    //
    // // You can also write to the child process's standard input, for example, sending input
    // let input = "Some input for the child process\n";
    // stdin
    //     .write_all(input.as_bytes())
    //     .expect("Failed to write to stdin");
    //
    // // You can handle the child process's standard error in a similar way
    //
    // // Wait for the child process to finish
    // let status = child.wait().expect("Failed to wait for child process");
    // println!("Child process exited with: {}", status);

    return Ok(());

    //

    let res = secrets::list(token, project, environment, None, false).await;

    if let Err(err) = res {
        debug!("Error: {:#?}", &err);

        return Ok(());
    }

    let res = res.unwrap();

    match res {
        GetRequestApiResponse::Ok(data) => {
            let secrets = serde_json::from_str::<Vec<Secret>>(&data.text);
            match secrets {
                Ok(secrets) => {
                    eprintln!("{:#?}", &secrets);

                    // inject_secrets(secrets);

                    let mut child = Command::new("npm")
                        .arg("run")
                        .arg("dev")
                        .spawn()
                        .expect("Failed to start Node.js server");

                    // Wait for the child process to finish
                    let status = child.wait().expect("Failed to wait for Node.js server");
                    if !status.success() {
                        eprintln!("Node.js server process exited with an error: {:?}", status);
                    }
                }
                Err(e) => {
                    panic!("{}", e);
                }
            }
        }
        GetRequestApiResponse::Err(e) => {
            panic!("{}", e);
        }
    }

    Ok(())
}

fn create_env_vars(secrets: Vec<Secret>) -> HashMap<String, String> {
    let mut map: HashMap<String, String> = HashMap::new();

    for secret in secrets {
        map.insert(secret.key, secret.value);
    }

    map
}
