use anyhow::Result;

use crate::{
    cmd::scans::{ScanCommands, ScanSubcommand},
    handlers::scans::{
        commits::{handle_scan_unpushed_commit_hunks, HandleScanUnpushedCommitHunksArgs},
        files::{
            handle_scan_changed_file_hunks, handle_scan_staged_file_hunks,
            HandleScanStagedFileHunksArgs,
        },
        install::{install_scan_hook, HookType},
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
            let hook_type = HookType::parse(&args.hook)?;
            install_scan_hook(hook_type, args.file.as_deref())?;
        }
    }

    Ok(())
}
