use anyhow::{Context, Result};
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

pub fn init_scan_config(file: Option<&str>, force: bool, silent: bool) -> Result<()> {
    let path_str = file.unwrap_or(DEFAULT_SCAN_CONFIG_PATH);
    let path = Path::new(path_str);

    if path.exists() && !force {
        if !silent {
            eprintln!();
        }
        eprintln!(
            "Scan config already exists at '{}'. Use --file to choose another path.",
            path.display()
        );
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
                if !silent {
                    eprintln!();
                }
                eprintln!("{err}");
                std::process::exit(1);
            }
        }
    }

    if let Err(err) = fs::write(path, SCAN_CONFIG_TEMPLATE)
        .with_context(|| format!("Failed to write scan config '{}'", path.display()))
    {
        if !silent {
            eprintln!();
        }
        eprintln!("{err}");
        std::process::exit(1);
    }

    if !silent {
        println!();
    }

    if force {
        println!("✔ Wrote scan config at {}", path.display());
    } else {
        println!("✔ Created scan config at {}", path.display());
    }
    println!("Tip: run 'stashbase scan staged -c {}'", path.display());

    Ok(())
}

pub fn validate_scan_config(file: Option<&str>, silent: bool) -> Result<()> {
    let path = file.unwrap_or(DEFAULT_SCAN_CONFIG_PATH);

    if let Err(err) = crate::models::scans::ScanConfig::load_from_file(path) {
        if !silent {
            eprintln!();
        }
        eprintln!("{err}");
        std::process::exit(1);
    }

    if !silent {
        println!();
    }
    println!("✔ Scan config is valid: {}", path);
    Ok(())
}
