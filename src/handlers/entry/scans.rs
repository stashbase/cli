use anyhow::Result;

use crate::{
    cmd::scans::{ScanCommands, ScanConfigSubcommand, ScanSubcommand},
    handlers::scans::{
        commits::{handle_scan_unpushed_commit_hunks, HandleScanUnpushedCommitHunksArgs},
        config::{init_scan_config, validate_scan_config},
        files::{
            handle_scan_changed_file_hunks, handle_scan_staged_file_hunks,
            HandleScanStagedFileHunksArgs,
        },
        install::{install_scan_hook, uninstall_scan_hook, HookType},
    },
};

pub async fn handle_scan_commands(
    cmd: ScanCommands,
    api_key: String,
    raw_output: bool,
    silent: bool,
) -> Result<()> {
    match cmd.subcommand {
        ScanSubcommand::Staged(args) => {
            let args = HandleScanStagedFileHunksArgs {
                api_key,
                silent,
                json_format: raw_output,
                excluded_files: args.exclude_files,
                baseline: args.baseline,
                output_dir: args.output_dir,
                config_file_path: args.config_file,
                ignore_secret_hashes: args.ignore_secret_hashes,
                ignore_secret_regexes: args.ignore_secret_regexes,
                match_environments: args.match_environments,
                match_project: args.match_project,
                match_files: args.match_files,
            };

            handle_scan_staged_file_hunks(args).await?;
        }
        ScanSubcommand::Changes(args) => {
            let args = HandleScanStagedFileHunksArgs {
                api_key,
                silent,
                json_format: raw_output,
                excluded_files: args.exclude_files,
                baseline: args.baseline,
                output_dir: args.output_dir,
                config_file_path: args.config_file,
                ignore_secret_hashes: args.ignore_secret_hashes,
                ignore_secret_regexes: args.ignore_secret_regexes,
                match_environments: args.match_environments,
                match_project: args.match_project,
                match_files: args.match_files,
            };

            handle_scan_changed_file_hunks(args).await?;
        }
        ScanSubcommand::Unpushed(args) => {
            let args = HandleScanUnpushedCommitHunksArgs {
                api_key,
                silent,
                json_format: raw_output,
                exclude_files: args.exclude_files,
                baseline: args.baseline,
                output_dir: args.output_dir,
                config_file_path: args.config_file,
                ignore_secret_hashes: args.ignore_secret_hashes,
                ignore_secret_regexes: args.ignore_secret_regexes,
                match_environments: args.match_environments,
                match_project: args.match_project,
                match_files: args.match_files,
                last_n_commits: args.last_n_commits,
            };

            handle_scan_unpushed_commit_hunks(args).await?;
        }
        ScanSubcommand::Install(args) => {
            if args.all {
                if args.file.is_some() {
                    anyhow::bail!("--file cannot be used with --all. Install hooks individually when using a custom file path.");
                }
                install_scan_hook(HookType::PreCommit, None, silent, raw_output)?;
                install_scan_hook(HookType::PrePush, None, silent, raw_output)?;
            } else {
                let hook = args
                    .hook
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("Hook is required unless --all is provided."))?;
                let hook_type = HookType::parse(hook)?;
                install_scan_hook(hook_type, args.file.as_deref(), silent, raw_output)?;
            }
        }
        ScanSubcommand::Uninstall(args) => {
            let hook_type = HookType::parse(&args.hook)?;
            uninstall_scan_hook(hook_type, args.file.as_deref(), silent, raw_output)?;
        }
        ScanSubcommand::Config(args) => match args.subcommand {
            ScanConfigSubcommand::Init(init_args) => {
                init_scan_config(
                    init_args.file.as_deref(),
                    init_args.force,
                    silent,
                    raw_output,
                )?;
            }
            ScanConfigSubcommand::Validate(validate_args) => {
                validate_scan_config(validate_args.config_file.as_deref(), silent, raw_output)?;
            }
        },
    }

    Ok(())
}
