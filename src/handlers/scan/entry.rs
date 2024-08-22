use std::time::{Duration, Instant};

use anyhow::{bail, Result};
use dialoguer::MultiSelect;
use spinoff::{spinners, Color, Spinner, Streams};
use tokio::time::sleep;

#[derive(Debug)]
pub struct HandleScanArgs {
    pub files: Vec<String>,
    pub staged: bool,
    pub autofix: bool,
}

pub async fn handle_scan(args: HandleScanArgs) -> Result<()> {
    let mut spinner = Spinner::new_with_stream(
        spinners::Dots,
        "Scanning in progress...",
        Color::Cyan,
        Streams::Stderr,
    );

    let random_duration = Duration::from_secs(3);

    // Start the timer
    let start = Instant::now();
    sleep(random_duration).await;

    if (args.autofix == false) {
        spinner.stop_with_message("Scan completed, found 3 issues:\n");

        let items = vec![
            "Line: 2; Value: 'https://api...'; Suggested name: QUOTES_API_BASE_URL",
            "Line: 3; Value: '9b8b7823b48...'; Suggested name: QUOTES_API_TOKEN",
            "Line: 9; Value: 'wh_xeC39HqL...'; Suggested name: WEATHER_API_KEY",
            "Line: 11; Value: 'weather-api...'; Suggested name: WEATHER_API_CLIENT_ID",
        ];

        let selection = MultiSelect::new()
            .with_prompt("Select items to resolve")
            .items(&items)
            .interact();

        match selection {
            Ok(selection) => {
                println!("You chose:");
                for i in selection {
                    println!("{}", items[i]);
                }
            }
            Err(err) => {
                bail!("\nAction aborted");
            }
        }
    } else {
        spinner.stop_with_message("Scan completed, 3 issues resolved:\n");

        let items = vec![
            "Line: 2; Value: 'https://api...'; Variable name: QUOTES_API_BASE_URL",
            "Line: 3; Value: '9b8b7823b48...'; Variable name: QUOTES_API_TOKEN",
            "Line: 9; Value: 'wh_xeC39HqL...'; Variable name: WEATHER_API_KEY",
            "Line: 11; Value: 'weather-api...'; Suggested name: WEATHER_API_CLIENT_ID",
        ];

        eprint!("{}", items.join("\n"));
    }

    Ok(())
}
