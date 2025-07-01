use anyhow::Result;

use crate::{
    cmd::scans::{ScanCommands, ScanSubcommand},
    handlers::scans::staged::{handle_scan_staged_file_hunks, HandleScanStagedFileHunksArgs},
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
                config_file_path: "stashbase-scan.yaml".to_string(),
            };

            handle_scan_staged_file_hunks(args).await?;
        }
        ScanSubcommand::Commits(scan_commits) => todo!(),
    }

    Ok(())
}
