use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use anyhow::{bail, Context, Result};

use crate::{
    cmd::generate::{GenerateSshKeyType, GenerateSshKeypair},
    utils::output::get_formatted_json_string,
};

pub fn handle_generate_ssh_keypair(args: GenerateSshKeypair, json_format: bool) -> Result<()> {
    if args.key_type != GenerateSshKeyType::Rsa && args.bits.is_some() {
        bail!("--bits can only be used with --type rsa");
    }

    let private_key_path = expand_home_path(&args.out);
    let public_key_path = private_key_path.with_extension("pub");

    if !args.force && (private_key_path.exists() || public_key_path.exists()) {
        bail!(
            "Refusing to overwrite existing key files. Use --force to overwrite: {}",
            private_key_path.display()
        );
    }

    if args.force {
        if private_key_path.exists() {
            fs::remove_file(&private_key_path).with_context(|| {
                format!("Failed to remove existing private key: {}", private_key_path.display())
            })?;
        }
        if public_key_path.exists() {
            fs::remove_file(&public_key_path).with_context(|| {
                format!("Failed to remove existing public key: {}", public_key_path.display())
            })?;
        }
    }

    create_parent_dir_if_needed(&private_key_path)?;
    generate_keypair(&args, &private_key_path)?;
    harden_permissions(&private_key_path, &public_key_path)?;

    let fingerprint = get_fingerprint(&public_key_path)?;
    let public_key = fs::read_to_string(&public_key_path)
        .with_context(|| format!("Failed to read public key: {}", public_key_path.display()))?;

    if json_format {
        let mut json = serde_json::json!({
            "private_key_path": private_key_path.to_string_lossy().to_string(),
            "public_key_path": public_key_path.to_string_lossy().to_string(),
            "fingerprint": fingerprint,
        });

        if args.print_public {
            json["public_key"] = serde_json::Value::String(public_key.trim().to_string());
        }

        let json_pretty = get_formatted_json_string(&json, true).unwrap();
        println!("{}", json_pretty);
    } else {
        println!("Private key: {}", private_key_path.display());
        println!("Public key: {}", public_key_path.display());
        println!("Fingerprint: {}", fingerprint);

        if args.print_public {
            println!("Public key value:");
            println!("{}", public_key.trim());
        }
    }

    Ok(())
}

fn generate_keypair(args: &GenerateSshKeypair, private_key_path: &Path) -> Result<()> {
    let mut command = Command::new("ssh-keygen");
    command
        .arg("-t")
        .arg(args.key_type.as_ssh_keygen_type())
        .arg("-f")
        .arg(private_key_path.as_os_str())
        .arg("-N")
        .arg(args.passphrase.clone().unwrap_or_default())
        .arg("-C")
        .arg(&args.comment)
        .arg("-q")
        .stdin(Stdio::null());

    if args.key_type == GenerateSshKeyType::Rsa {
        command.arg("-b").arg(args.bits.unwrap_or(4096).to_string());
    }

    let output = command
        .output()
        .context("Failed to execute ssh-keygen. Make sure ssh-keygen is installed.")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let error_message = if stderr.is_empty() {
            "ssh-keygen failed without error output.".to_string()
        } else {
            format!("ssh-keygen failed: {}", stderr)
        };
        bail!(error_message);
    }

    Ok(())
}

fn get_fingerprint(public_key_path: &Path) -> Result<String> {
    let output = Command::new("ssh-keygen")
        .arg("-lf")
        .arg(public_key_path.as_os_str())
        .output()
        .context("Failed to read key fingerprint using ssh-keygen.")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let error_message = if stderr.is_empty() {
            "Failed to read SSH key fingerprint.".to_string()
        } else {
            format!("Failed to read SSH key fingerprint: {}", stderr)
        };
        bail!(error_message);
    }

    let line = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if line.is_empty() {
        bail!("Unable to parse fingerprint from ssh-keygen output.");
    }

    Ok(line)
}

fn create_parent_dir_if_needed(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
    }

    Ok(())
}

fn expand_home_path(path: &str) -> PathBuf {
    if path == "~" {
        return std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(path));
    }

    if let Some(relative) = path.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(relative);
        }
    }

    PathBuf::from(path)
}

fn harden_permissions(private_key_path: &Path, public_key_path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        fs::set_permissions(private_key_path, fs::Permissions::from_mode(0o600)).with_context(
            || {
                format!(
                    "Failed to set permissions on private key: {}",
                    private_key_path.display()
                )
            },
        )?;
        fs::set_permissions(public_key_path, fs::Permissions::from_mode(0o644)).with_context(
            || {
                format!(
                    "Failed to set permissions on public key: {}",
                    public_key_path.display()
                )
            },
        )?;
    }

    Ok(())
}
