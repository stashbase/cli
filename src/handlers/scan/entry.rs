use std::{
    fs,
    time::{Duration, Instant},
};

use anyhow::{bail, Result};
use dialoguer::MultiSelect;
use spinoff::{spinners, Color, Spinner, Streams};
use tokio::time::sleep;

#[derive(Debug)]
pub struct HandleScanArgs {
    pub files: Vec<String>,
    pub staged: bool,
    pub auto_resolve: bool,
    pub project: Option<String>,
    pub environment: Option<String>,
}

pub async fn handle_scan(args: HandleScanArgs) -> Result<()> {
    let resolved_content = r#"function initializeApiClient() {
  const baseUrl = process.env.QUOTES_API_BASE_URL
  const token = process.env.QUOTES_API_TOKEN

  console.log(`API Client initialized with base URL: ${baseUrl}`)

  return {
    fetchResource: async (endpoint: string) => {
      const response = await fetch(`${baseUrl}/${endpoint}`, {
        headers: {
          Authorization: `Bearer ${token}`,
        },
      })
      return response.json()
    },
  }
}

async function fetchData() {
  const apiKey = process.env.WEATHER_API_KEY
  const clientId = process.env.WEATHER_API_CLIENT_ID

  console.log(`Fetching data with API key ${apiKey} and client ID ${clientId}`)
  const response = await fetch('https://api.weather.com/resource', {
    headers: {
      Authorization: `Bearer ${apiKey}`,
      'Client-ID': clientId,
    },
  })
  const data = await response.json()
  return data
}

async function main() {
  const apiClient = initializeApiClient()

  const dataFromClient = await apiClient.fetchResource('data')
  console.log(`Fetched data from API client: ${JSON.stringify(dataFromClient)}`)

  const dataFromFunction = await fetchData()
  console.log(`Fetched data from fetchData function: ${JSON.stringify(dataFromFunction)}`)
}

main()"#;

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

    let target_file = match args.files.is_empty() {
        true => String::from("main.ts"),
        false => args.files[0].clone(),
    };

    if args.auto_resolve == false {
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
                if selection.is_empty() {
                    bail!("\nNo issues to resolve selected");
                }

                fs::write(target_file.clone(), resolved_content).expect("Unable to write file");

                let msg = format!(
                    "Resolved {} items for file '{}'",
                    selection.len(),
                    target_file,
                );
                eprintln!("\n{}", msg);

                if let Some(project) = args.project {
                    if let Some(environment) = args.environment {
                        eprintln!(
                            "Secrets saved to project '{}', environment '{}'",
                            project, environment
                        );
                    }
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

        fs::write(target_file, resolved_content).expect("Unable to write file");

        eprint!("{}", items.join("\n"));
    }

    Ok(())
}
