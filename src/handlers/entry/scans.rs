use anyhow::Result;

use crate::{
    cmd::scans::{ScanCommands, ScanSubcommand},
    handlers::scans::{
        commits::{handle_scan_unpushed_commit_hunks, HandleScanUnpushedCommitHunksArgs},
        staged::{handle_scan_staged_file_hunks, HandleScanStagedFileHunksArgs},
    },
};

pub async fn handle_scan_commands(
    cmd: ScanCommands,
    api_key: String,
    raw_output: bool,
) -> Result<()> {
    match cmd.subcommand {
        ScanSubcommand::Staged(args) => {
            let args = HandleScanStagedFileHunksArgs {
                api_key,
                json_format: raw_output,
                exclude: args.exclude,
                output_dir: args.output_dir,
                config_file_path: args.config_file,
                ignore_value_hashes: args.ignore_value_hashes,
            };

            handle_scan_staged_file_hunks(args).await?;
        }
        ScanSubcommand::Commits(args) => {
            let args = HandleScanUnpushedCommitHunksArgs {
                api_key,
                json_format: raw_output,
                exclude: args.exclude,
                output_dir: args.output_dir,
                config_file_path: args.config_file,
                ignore_value_hashes: args.ignore_value_hashes,
            };

            handle_scan_unpushed_commit_hunks(args).await?;
        }
    }

    Ok(())
}
