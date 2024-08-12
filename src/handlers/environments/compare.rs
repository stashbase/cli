use anyhow::{bail, Result};
use colored_json::to_colored_json_auto;
use log::debug;

use tabled::{
    builder::Builder,
    settings::style::Style,
    settings::{object::Rows, peaker::PriorityMax, Color, Modify, Settings, Width},
};

use crate::{
    api::environments::{self, CompareEnvironmentsRequestArgs},
    models::{api_client::GetRequestApiResponse, environments::CompareEnvironmentsResponse},
    utils::{
        spinner::request_spinner,
        term_size::get_terminal_size,
        validation::{validate_project_environment_identifier, validate_project_identifier},
    },
};

pub struct HandleCompareEnvironmentsArgs {
    pub api_key: String,
    pub project: String,
    pub environment_1: String,
    pub environment_2: String,
    pub only_keys: bool,
    pub json_format: bool,
}

pub async fn handle_compare_environments(args: HandleCompareEnvironmentsArgs) -> Result<()> {
    let validation_res =
        validate_project_environment_identifier(&args.project, &args.environment_1, false);

    if let Err(err) = validation_res {
        bail!(err);
    }

    let env_identifier_validation_res = validate_project_identifier(&args.environment_2, false);

    if let Err(err) = env_identifier_validation_res {
        bail!(err);
    }

    let mut spinner = request_spinner();

    let req_args = CompareEnvironmentsRequestArgs {
        api_key: args.api_key,
        project: args.project,
        environment_1: &args.environment_1,
        environment_2: &args.environment_2,
        only_keys: &args.only_keys,
    };

    let res = environments::compare(req_args).await;

    if let Err(err) = res {
        spinner.stop_and_persist("", "");
        debug!("Error: {:#?}", &err);
        bail!(err);
    }

    let res = res.unwrap();

    match res {
        GetRequestApiResponse::Ok(data) => {
            let data = serde_json::from_str::<CompareEnvironmentsResponse>(&data.text);

            match data {
                Ok(data) => {
                    debug!("{:#?}", &data);

                    if data.is_empty() {
                        spinner.stop_with_message("No secrets to compare");
                    } else {
                        spinner.stop_and_persist("", "");

                        let print_string = format_comparison(
                            args.environment_1,
                            args.environment_2,
                            data,
                            args.json_format,
                            args.only_keys,
                        );

                        println!("{}", print_string);
                    }
                }
                Err(_) => {
                    spinner.stop_and_persist("", "");
                    bail!("Something went wrong")
                }
            }
        }
        GetRequestApiResponse::Err(e) => {
            spinner.stop_and_persist("", "");
            bail!("{}", e);
        }
    }

    Ok(())
}

fn format_comparison(
    environment_1: String,
    environment_2: String,
    data: CompareEnvironmentsResponse,
    json_format: bool,
    only_keys: bool,
) -> String {
    if json_format {
        let value = serde_json::to_value(&data).unwrap();
        let pretty = to_colored_json_auto(&value).unwrap();
        return pretty;
    } else {
        let mut table_data = vec![vec![String::from("Key"), environment_1, environment_2]];

        for row in data {
            let value_1 = row.values.get(0).cloned().unwrap();
            let value_2 = row.values.get(1).cloned().unwrap();

            let formatted_1 = get_formatted_table_value(value_1, only_keys);
            let formatted_2 = get_formatted_table_value(value_2, only_keys);

            table_data.push(vec![row.key, formatted_1, formatted_2]);
        }

        let mut table = Builder::from(table_data).build();

        let (width, _) = get_terminal_size();

        let term_size_settings = Settings::default()
            .with(Style::rounded())
            .with(Width::wrap(width).priority::<PriorityMax>())
            .with(Modify::new(Rows::first()).with(Color::FG_GREEN));

        table.with(term_size_settings);

        return format!("{table}");
    }
}

fn get_formatted_table_value(value: Option<String>, only_keys: bool) -> String {
    match value {
        Some(v) => {
            if only_keys {
                "•••••••••••".to_string()
            } else {
                match v == "" {
                    true => "\"\"".to_string(),
                    false => v,
                }
            }
        }
        None => "".to_string(),
    }
}
