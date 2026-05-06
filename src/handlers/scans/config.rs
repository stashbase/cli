use anyhow::{bail, Context, Result};
use std::{fs, path::Path};

const DEFAULT_SCAN_CONFIG_PATH: &str = "stashbase-scan.yaml";
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

pub fn init_scan_config(file: Option<&str>, force: bool) -> Result<()> {
    let path_str = file.unwrap_or(DEFAULT_SCAN_CONFIG_PATH);
    let path = Path::new(path_str);

    if path.exists() && !force {
        bail!(
            "Scan config already exists at '{}'. Use --file to choose another path.",
            path.display()
        );
    }

    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).with_context(|| {
                format!(
                    "Failed to create directory for scan config at '{}'",
                    parent.display()
                )
            })?;
        }
    }

    fs::write(path, SCAN_CONFIG_TEMPLATE)
        .with_context(|| format!("Failed to write scan config '{}'", path.display()))?;

    if force {
        println!("✔ Wrote scan config at {}", path.display());
    } else {
        println!("✔ Created scan config at {}", path.display());
    }
    println!("Tip: run 'stashbase scan staged -c {}'", path.display());

    Ok(())
}
