use std::{
    env, fs,
    path::{Path, PathBuf},
};

use anyhow::Result;
use serde::Serialize;

use crate::{
    api::auth::get_current_auth_details_no_retry,
    cmd::doctor::DoctorCommand,
    config::{config, secure_store},
    models::{api_client::GetRequestApiResponse, config::Config},
    utils::{
        env::get_stashbase_api_key,
        output::{get_formatted_json_string, ColorizeIfColoredOutput},
    },
};

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum DoctorStatus {
    Ok,
    Warn,
    Fail,
}

#[derive(Debug, Serialize)]
struct DoctorCheck {
    name: String,
    status: DoctorStatus,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<String>,
}

#[derive(Debug, Serialize)]
struct DoctorReport {
    status: DoctorStatus,
    checks: Vec<DoctorCheck>,
}

pub async fn handle_doctor_command(
    cmd: DoctorCommand,
    json_format: bool,
    api_key_override: Option<String>,
) -> Result<bool> {
    let mut checks: Vec<DoctorCheck> = Vec::new();
    let mut parsed_config: Option<Config> = None;
    let mut config_path: Option<PathBuf> = None;

    match config::get_config_path() {
        Ok(path) => {
            config_path = Some(path.clone());
            if cmd.verbose {
                checks.push(okd(
                    "Config path",
                    "Resolved",
                    format!("Path: {}", path.display()),
                ));
            } else {
                checks.push(ok("Config path", format!("Resolved: {}", path.display())));
            }

            if path.exists() {
                match fs::read_to_string(&path) {
                    Ok(content) => match toml::from_str::<Config>(&content) {
                        Ok(cfg) => {
                            parsed_config = Some(cfg);
                            checks
                                .push(ok("Config file", "config.toml is readable and valid TOML"));
                        }
                        Err(err) => {
                            checks.push(fail("Config file", format!("Invalid TOML: {}", err)))
                        }
                    },
                    Err(err) => checks.push(fail(
                        "Config file",
                        format!("Failed to read {}: {}", path.display(), err),
                    )),
                }
            } else {
                checks.push(warn(
                    "Config file",
                    format!(
                        "Not found at {} (it will be created on demand)",
                        path.display()
                    ),
                ));
            }
        }
        Err(err) => checks.push(fail(
            "Config path",
            format!("Cannot resolve config path: {}", err),
        )),
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        if let Some(path) = &config_path {
            if let Some(config_dir) = path.parent() {
                if let Ok(metadata) = fs::metadata(config_dir) {
                    let mode = metadata.permissions().mode() & 0o777;
                    if mode == 0o700 {
                        checks.push(ok("Config dir permissions", format!("{:o}", mode)));
                    } else {
                        checks.push(warn(
                            "Config dir permissions",
                            format!("Expected 700, got {:o}", mode),
                        ));
                    }
                }
            }

            if path.exists() {
                if let Ok(metadata) = fs::metadata(path) {
                    let mode = metadata.permissions().mode() & 0o777;
                    if mode == 0o600 {
                        checks.push(ok("Config file permissions", format!("{:o}", mode)));
                    } else {
                        checks.push(warn(
                            "Config file permissions",
                            format!("Expected 600, got {:o}", mode),
                        ));
                    }
                }
            }
        }
    }

    let env_api_key = get_stashbase_api_key();
    let secure_store_api_key = match secure_store::get_api_key() {
        Ok(key) => {
            checks.push(ok("Secure store", "Accessible"));
            key
        }
        Err(err) => {
            checks.push(warn(
                "Secure store",
                format!("Could not read secure store API key: {}", err),
            ));
            None
        }
    };

    let legacy_config_api_key = parsed_config.as_ref().and_then(|cfg| cfg.api_key.clone());

    let api_key_presence_details = if cmd.verbose {
        Some(format!(
            "Sources available: --api-key={}, env(STASHBASE_API_KEY)={}, secure_store={}, legacy_config={}",
            api_key_override.is_some(),
            env_api_key.is_some(),
            secure_store_api_key.is_some(),
            legacy_config_api_key.is_some()
        ))
    } else {
        None
    };

    let selected_api_key = if let Some(cli_key) = api_key_override {
        checks.push(with_optional_details(
            ok("API key source", "Using --api-key flag"),
            api_key_presence_details.clone(),
        ));
        Some(cli_key)
    } else if let Some(key) = env_api_key {
        checks.push(with_optional_details(
            ok(
                "API key source",
                "Using STASHBASE_API_KEY environment variable",
            ),
            api_key_presence_details.clone(),
        ));
        Some(key)
    } else if let Some(key) = secure_store_api_key {
        checks.push(with_optional_details(
            ok("API key source", "Using secure store key"),
            api_key_presence_details.clone(),
        ));
        Some(key)
    } else if let Some(key) = legacy_config_api_key {
        checks.push(with_optional_details(
            warn(
                "API key source",
                "Using legacy config file key; run setup/config to migrate to secure store",
            ),
            api_key_presence_details.clone(),
        ));
        Some(key)
    } else {
        checks.push(with_optional_details(
            warn("API key source", "No API key found"),
            api_key_presence_details.clone(),
        ));
        None
    };

    checks.push(check_binary("git", cmd.verbose));
    checks.push(check_binary("ssh-keygen", cmd.verbose));

    if cmd.auth_check {
        match selected_api_key {
            Some(api_key) => {
                let auth_response = get_current_auth_details_no_retry(api_key).await;
                match auth_response {
                    Ok(GetRequestApiResponse::Ok(_)) => {
                        checks.push(ok("Auth check", "API authentication succeeded"));
                    }
                    Ok(GetRequestApiResponse::Err(err)) => {
                        checks.push(fail(
                            "Auth check",
                            format!("API rejected credentials: {}", err),
                        ));
                    }
                    Err(err) => {
                        checks.push(fail("Auth check", format!("Failed to reach API: {}", err)));
                    }
                }
            }
            None => checks.push(warn(
                "Auth check",
                "Skipped: no API key available to validate",
            )),
        }
    } else {
        checks.push(ok(
            "Auth check",
            "Skipped (enable with --auth-check for live API validation)",
        ));
    }

    let status = overall_status(&checks);
    let report = DoctorReport { status, checks };

    if json_format {
        let json = get_formatted_json_string(&report, true).unwrap();
        println!("{}", json);
    } else {
        println!("Stashbase CLI Doctor\n");
        for check in &report.checks {
            let label = match check.status {
                DoctorStatus::Ok => "OK".green_if_tty(),
                DoctorStatus::Warn => "WARN".yellow_if_tty(),
                DoctorStatus::Fail => "FAIL".red_if_tty(),
            };
            println!("[{}] {}: {}", label, check.name, check.message);
            if cmd.verbose {
                if let Some(details) = &check.details {
                    println!("    {}", details);
                }
            }
        }

        println!();
        let summary = match report.status {
            DoctorStatus::Ok => "Doctor finished: all checks passed.".green_if_tty(),
            DoctorStatus::Warn => "Doctor finished: warnings detected.".yellow_if_tty(),
            DoctorStatus::Fail => "Doctor finished: failures detected.".red_if_tty(),
        };
        println!("{}", summary);
    }

    Ok(report.status == DoctorStatus::Fail)
}

fn check_binary(binary: &str, verbose: bool) -> DoctorCheck {
    if let Some(path) = command_path(binary) {
        if verbose {
            okd(
                format!("Binary `{}`", binary),
                "Found in PATH",
                format!("Resolved executable: {}", path.display()),
            )
        } else {
            ok(format!("Binary `{}`", binary), "Found in PATH")
        }
    } else {
        warn(
            format!("Binary `{}`", binary),
            "Not found in PATH (some commands may fail)",
        )
    }
}

fn command_path(binary: &str) -> Option<PathBuf> {
    let Some(path_var) = env::var_os("PATH") else {
        return None;
    };

    let candidates = binary_candidates(binary);

    for dir in env::split_paths(&path_var) {
        for candidate in &candidates {
            let full_path = dir.join(candidate);
            if is_executable_file(&full_path) {
                return Some(full_path);
            }
        }
    }

    None
}

fn binary_candidates(binary: &str) -> Vec<String> {
    #[cfg(windows)]
    {
        let mut candidates = vec![binary.to_string()];

        if let Some(pathext) = env::var_os("PATHEXT") {
            for ext in pathext.to_string_lossy().split(';') {
                let ext = ext.trim();
                if !ext.is_empty() {
                    candidates.push(format!("{}{}", binary, ext));
                }
            }
        }

        candidates
    }

    #[cfg(not(windows))]
    {
        vec![binary.to_string()]
    }
}

fn is_executable_file(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(metadata) = fs::metadata(path) {
            return metadata.permissions().mode() & 0o111 != 0;
        }
    }

    #[cfg(not(unix))]
    {
        return true;
    }

    false
}

fn overall_status(checks: &[DoctorCheck]) -> DoctorStatus {
    if checks
        .iter()
        .any(|check| check.status == DoctorStatus::Fail)
    {
        DoctorStatus::Fail
    } else if checks
        .iter()
        .any(|check| check.status == DoctorStatus::Warn)
    {
        DoctorStatus::Warn
    } else {
        DoctorStatus::Ok
    }
}

fn ok(name: impl Into<String>, message: impl Into<String>) -> DoctorCheck {
    DoctorCheck {
        name: name.into(),
        status: DoctorStatus::Ok,
        message: message.into(),
        details: None,
    }
}

fn warn(name: impl Into<String>, message: impl Into<String>) -> DoctorCheck {
    DoctorCheck {
        name: name.into(),
        status: DoctorStatus::Warn,
        message: message.into(),
        details: None,
    }
}

fn fail(name: impl Into<String>, message: impl Into<String>) -> DoctorCheck {
    DoctorCheck {
        name: name.into(),
        status: DoctorStatus::Fail,
        message: message.into(),
        details: None,
    }
}

fn okd(
    name: impl Into<String>,
    message: impl Into<String>,
    details: impl Into<String>,
) -> DoctorCheck {
    DoctorCheck {
        name: name.into(),
        status: DoctorStatus::Ok,
        message: message.into(),
        details: Some(details.into()),
    }
}

fn with_optional_details(mut check: DoctorCheck, details: Option<String>) -> DoctorCheck {
    check.details = details;
    check
}
