use anyhow::{bail, Result};
use colored_json::to_colored_json_auto;
use log::debug;

use crate::{
    api::auth::get_current_auth_details,
    cmd::config::OutputFormat,
    models::{api_client::GetRequestApiResponse, auth::CurrentAuthResponse},
    utils::spinner::request_spinner,
};

// for whoami command
pub struct GetCurrentAuthDetailsRequestArgs {
    pub api_key: String,
    pub format: OutputFormat,
}

pub async fn handle_whoami_command(args: GetCurrentAuthDetailsRequestArgs) -> Result<()> {
    let args = GetCurrentAuthDetailsRequestArgs {
        api_key: args.api_key,
        format: args.format,
    };

    let json_format = args.format == OutputFormat::Json;
    let mut spinner = request_spinner();

    let response = get_current_auth_details(args.api_key).await;

    if let Err(err) = response {
        spinner.stop_and_persist("", "");

        let formatted_err = err.format_error_output(json_format)?;

        eprintln!();
        bail!(formatted_err);
    }

    let response = response.unwrap();

    match response {
        GetRequestApiResponse::Ok(data) => {
            let auth_details = serde_json::from_str::<CurrentAuthResponse>(&data.text);
            spinner.stop_and_persist("", "");

            match auth_details {
                Ok(auth_details) => match args.format {
                    OutputFormat::Json => {
                        let value = serde_json::to_value(&auth_details).unwrap();
                        let pretty = to_colored_json_auto(&value).unwrap();
                        println!("{}", pretty);
                    }
                    OutputFormat::List => {
                        print!("{}", auth_details);
                    }
                    OutputFormat::Table => {
                        unreachable!()
                    }
                },
                Err(err) => {
                    spinner.stop_and_persist("", "");
                    debug!("Error: {:#?}", &err);
                    bail!(err);
                }
            }
        }
        GetRequestApiResponse::Err(err) => {
            spinner.stop_and_persist("", "");

            let formatted_err = err.format_error_output(json_format)?;

            eprintln!();
            bail!(formatted_err);
        }
    }

    Ok(())
}
