use anyhow::{bail, Result};
use colored_json::to_colored_json_auto;
use log::{debug, error};

use crate::{
    api::projects,
    cmd::{config::OutputFormat, projects::SortBy},
    models::{
        api_client::{GetRequestApiResponse, OutputError},
        projects::{
            ProjectList, ProjectWithCountNoDescriptionTable, SingleListProject,
            SingleListProjectWithoutDescription,
        },
        shared::PaginationMetadata,
        validation::{InputValidationError, ProjectInputValidationError},
    },
    utils::{
        human_datetime::get_human_datetime, spinner::request_spinner, tables,
        validation::validate_project_search,
    },
};

pub struct HandleListProjectsArgs {
    pub api_key: String,
    pub search: Option<String>,
    pub sort_by: Option<SortBy>,
    pub descending: bool,
    pub page: Option<usize>,
    pub limit: Option<usize>,
    pub format: OutputFormat,
    pub silent: bool,
}

pub async fn handle_list_projects(args: HandleListProjectsArgs) -> Result<()> {
    let HandleListProjectsArgs {
        api_key,
        search,
        sort_by,
        descending,
        format,
        page,
        limit,
        silent,
    } = args;

    // validate search
    if let Some(search) = &search {
        let search_validation_res = validate_project_search(&search);

        if let Err(err) = search_validation_res {
            let error_output = err.format_error_output(format == OutputFormat::Json)?;

            if !silent {
                eprintln!();
            }

            bail!(error_output);
        }
    }

    if let Some(limit) = limit {
        if limit < 2 || limit > 30 {
            let error = InputValidationError::Projects(ProjectInputValidationError::InvalidLimit);
            let error_output = error.format_error_output(format == OutputFormat::Json)?;

            if !silent {
                eprintln!();
            }

            bail!(error_output);
        }
    }

    if let Some(page) = page {
        if page < 1 || page > 1000 {
            let error = InputValidationError::Projects(ProjectInputValidationError::InvalidPage);
            let error_output = error.format_error_output(format == OutputFormat::Json)?;

            if !silent {
                eprintln!();
            }

            bail!(error_output);
        }
    }

    debug!("listing projects...:");

    let spinner = if !silent {
        Some(request_spinner())
    } else {
        None
    };

    let project_res = projects::list_projects(
        api_key,
        search,
        sort_by.unwrap_or_default(),
        descending,
        page,
        limit,
    )
    .await;

    if let Err(err) = project_res {
        error!("{:#?}", &err);

        if let Some(mut spinner) = spinner {
            spinner.stop_and_persist("", "");
        }

        let error_output = err.format_error_output(format == OutputFormat::Json)?;
        bail!(error_output);
    }

    let project_res = project_res.unwrap();

    match project_res {
        GetRequestApiResponse::Ok(data) => {
            debug!("{:#?}", &data.text);
            let data = serde_json::from_str::<ProjectList>(&data.text);

            match data {
                Ok(data) => {
                    debug!("{:#?}", &data);

                    if let OutputFormat::Json = format {
                        if let Some(mut spinner) = spinner {
                            spinner.stop_and_persist("", "");
                        }

                        output_json(&data);
                        return Ok(());
                    }

                    let projects = data.data;
                    let pagination = data.pagination;

                    if projects.is_empty() {
                        if !silent {
                            if let Some(mut spinner) = spinner {
                                spinner.stop_with_message("No projects found.");
                            }

                            eprintln!("\n{}", pagination);
                        } else {
                            if let Some(mut spinner) = spinner {
                                spinner.stop_and_persist("", "");
                            }
                        }
                    } else {
                        if let Some(mut spinner) = spinner {
                            spinner.stop_and_persist("", "");
                        }

                        match format {
                            OutputFormat::List => {
                                output_list(projects, pagination);
                            }
                            OutputFormat::Table => {
                                // reverse because returned fro list -> last is first (for
                                // lists)
                                output_table(projects, pagination);
                            }
                            OutputFormat::Json => unreachable!(),
                        }
                    }
                }
                Err(e) => {
                    debug!("{:#?}", &e);

                    if let Some(mut spinner) = spinner {
                        spinner.stop_and_persist("", "");
                    }

                    let error = OutputError::failed_to_deserialize_response_body();
                    let formatted_err = error.format_error_output(format == OutputFormat::Json)?;

                    eprintln!();
                    bail!(formatted_err);
                }
            }
        }
        GetRequestApiResponse::Err(e) => {
            if let Some(mut spinner) = spinner {
                spinner.stop_and_persist("", "");
            }

            let error_output = e.format_error_output(format == OutputFormat::Json)?;
            bail!(error_output);
        }
    }

    Ok(())
}

fn output_list(projects: Vec<SingleListProject>, pagination: PaginationMetadata) {
    for (i, p) in projects.iter().enumerate() {
        if i == projects.len() - 1 {
            print!("{}", p);
        } else {
            println!("{}", p);
        }
    }

    eprintln!("\n\n{}", pagination);
}

fn output_json(data: &ProjectList) {
    let value = serde_json::to_value(data).unwrap();
    let pretty = to_colored_json_auto(&value).unwrap();

    println!("{}", pretty);
}

fn output_table(projects: Vec<SingleListProject>, pagination: PaginationMetadata) {
    let has_description = projects.iter().any(|p| p.description.is_some());
    if has_description {
        let projects_formatted: Vec<_> = projects
            .into_iter()
            .map(|mut p| {
                let (formatted, relative) = get_human_datetime(&p.created_at);
                p.created_at = format!("{} ({})", formatted, relative);
                p
            })
            .collect();

        let table = tables::build::build_table(&projects_formatted);
        println!("{}", table);
    } else {
        let projects_formatted: Vec<_> = projects
            .into_iter()
            .map(|mut p| {
                let (formatted, relative) = get_human_datetime(&p.created_at);
                p.created_at = format!("{} ({})", formatted, relative);
                SingleListProjectWithoutDescription::from(p)
            })
            .collect();

        let table = tables::build::build_table(&projects_formatted);
        println!("{}", table);
    }

    eprintln!("\n{}", pagination);
}
