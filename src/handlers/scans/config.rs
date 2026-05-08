use crate::{models::validation::InputValidationError, utils::output::get_formatted_json_string};
use anyhow::{Context, Result};
use git2::Repository;
use std::{fs, path::Path};

pub const DEFAULT_SCAN_CONFIG_PATH: &str = "stashbase-scan.yaml";
const SCAN_CONFIG_TEMPLATE: &str = r#"# Stashbase scan config
# Docs: stashbase scan staged -c stashbase-scan.yaml
#
# Patterns to exclude from scanning
excluded-files:
  - "node_modules/**"
  - "dist/**"

# Output directory for local scan result files
output-dir: ".stashbase/scans"

# Ignore known secret values by hash or regex
ignored-secrets:
  hashes: []
  regexes: []

# Optional secret matching context
match:
  project:
    # Project name or ID
    identifier: null
    # Environment names/IDs/folder selectors
    environments: []
  # Local files to match potential secrets against
  files: []
"#;

pub fn init_scan_config(
    file: Option<&str>,
    force: bool,
    silent: bool,
    json_format: bool,
) -> Result<()> {
    let path_str = file.unwrap_or(DEFAULT_SCAN_CONFIG_PATH);
    let path = Path::new(path_str);

    if path.exists() && !force {
        let message = format!(
            "Scan config already exists at '{}'. Use --file to choose another path.",
            path.display()
        );
        if json_format {
            let payload = serde_json::json!({ "message": message });
            let formatted = get_formatted_json_string(&payload, false)?;
            if !silent {
                eprintln!();
            }
            eprintln!("{formatted}");
        } else {
            if !silent {
                eprintln!();
            }
            eprintln!("{message}");
        }
        std::process::exit(1);
    }

    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            if let Err(err) = fs::create_dir_all(parent).with_context(|| {
                format!(
                    "Failed to create directory for scan config at '{}'",
                    parent.display()
                )
            }) {
                if json_format {
                    let payload = serde_json::json!({ "message": err.to_string() });
                    let formatted = get_formatted_json_string(&payload, false)?;
                    if !silent {
                        eprintln!();
                    }
                    eprintln!("{formatted}");
                } else {
                    if !silent {
                        eprintln!();
                    }
                    eprintln!("{err}");
                }
                std::process::exit(1);
            }
        }
    }

    if let Err(err) = fs::write(path, SCAN_CONFIG_TEMPLATE)
        .with_context(|| format!("Failed to write scan config '{}'", path.display()))
    {
        if json_format {
            let payload = serde_json::json!({ "message": err.to_string() });
            let formatted = get_formatted_json_string(&payload, false)?;
            if !silent {
                eprintln!();
            }
            eprintln!("{formatted}");
        } else {
            if !silent {
                eprintln!();
            }
            eprintln!("{err}");
        }
        std::process::exit(1);
    }

    if !silent {
        if json_format {
            let message = if force {
                format!("Wrote scan config at {}", path.display())
            } else {
                format!("Created scan config at {}", path.display())
            };
            let payload = serde_json::json!({
                "message": message,
                "path": path.display().to_string(),
                "overwritten": force
            });
            let pretty = get_formatted_json_string(&payload, false)?;
            println!("\n{}", pretty);
        } else {
            println!();
            if force {
                println!("✔ Wrote scan config at {}", path.display());
            } else {
                println!("✔ Created scan config at {}", path.display());
            }
            println!("Tip: run 'stashbase scan staged -c {}'", path.display());
        }
    }

    Ok(())
}

pub fn validate_scan_config(file: Option<&str>, silent: bool, json_format: bool) -> Result<()> {
    let path = file.unwrap_or(DEFAULT_SCAN_CONFIG_PATH);

    if let Err(err) = crate::models::scans::ScanConfig::load_from_file(path) {
        let input_error = InputValidationError::Scan(err);
        let output = input_error.format_error_output(json_format)?;
        if !silent && !json_format {
            eprintln!();
        }
        eprintln!("{output}");
        std::process::exit(1);
    }

    if !silent {
        if json_format {
            let message = serde_json::json!({
                "message": format!("Scan config is valid: {}", path)
            });
            let pretty = get_formatted_json_string(&message, false)?;
            println!("\n{}", pretty);
        } else {
            println!();
            println!("✔ Scan config is valid: {}", path);
        }
    } else if json_format {
        let message = serde_json::json!({
            "message": format!("Scan config is valid: {}", path)
        });
        let pretty = get_formatted_json_string(&message, false)?;
        println!("{}", pretty);
    }

    Ok(())
}

pub fn resolve_scan_config_path(config_file_path: Option<String>) -> Option<String> {
    if config_file_path.is_some() {
        return config_file_path;
    }

    let repo = Repository::discover(".").ok()?;
    let workdir = repo.workdir()?;
    let default_config = workdir.join(DEFAULT_SCAN_CONFIG_PATH);

    if default_config.exists() {
        Some(default_config.to_string_lossy().to_string())
    } else {
        None
    }
}
