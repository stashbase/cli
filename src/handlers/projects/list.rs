use anyhow::{bail, Result};
use log::debug;

use crate::{
    api::projects,
    cmd::{config::OutputFormat, projects::SortBy},
    models::{
        api_client::{GetRequestApiResponse, OutputError},
        projects::{ProjectList, SingleListProject, SingleListProjectWithoutDescription},
        shared::PaginationMetadata,
        validation::{InputValidationError, ProjectInputValidationError},
    },
    utils::human_datetime::get_human_datetime,
    utils::{
        output::get_formatted_json_string, spinner::request_spinner, tables,
        validation::validate_project_search,
    },
};

pub struct HandleListProjectsArgs {
    pub api_key: String,
    pub search: Option<String>,
    pub sort_by: Option<SortBy>,
    pub descending: bool,
    pub page: Option<usize>,
    pub page_size: Option<usize>,
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
        page_size,
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

    if let Some(page_size) = page_size {
        if page_size < 2 || page_size > 30 {
            let error =
                InputValidationError::Projects(ProjectInputValidationError::InvalidPageSize);
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

    let project_res =
        projects::list_projects(api_key, search, sort_by, descending, page, page_size).await;

    if let Err(err) = project_res {
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

                        let pretty = get_formatted_json_string(&data, true).unwrap();
                        println!("{}", pretty);

                        return Ok(());
                    }

                    let projects = data.data;
                    let pagination = data.pagination;

                    if projects.is_empty() {
                        if !silent {
                            if let Some(mut spinner) = spinner {
                                spinner.stop_with_message("No projects found.");
                            }

                            println!("\n{}", pagination);
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
                            OutputFormat::Plain => {
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

                    if !silent {
                        eprintln!();
                    }

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

    println!("\n\n{}", pagination);
}

fn output_table(projects: Vec<SingleListProject>, pagination: PaginationMetadata) {
    let has_description = projects.iter().any(|p| p.description.is_some());
    if has_description {
        let projects_formatted: Vec<_> = projects
            .into_iter()
            .map(|mut p| {
                let (formatted, _) = get_human_datetime(&p.created_at);
                p.created_at = formatted;
                let (formatted_updated, _) = get_human_datetime(&p.updated_at);
                p.updated_at = formatted_updated;
                p
            })
            .collect();

        let table = tables::build::build_table(&projects_formatted);
        println!("{}", table);
    } else {
        let projects_formatted: Vec<_> = projects
            .into_iter()
            .map(|mut p| {
                let (formatted, _) = get_human_datetime(&p.created_at);
                p.created_at = formatted;
                let (formatted_updated, _) = get_human_datetime(&p.updated_at);
                p.updated_at = formatted_updated;
                SingleListProjectWithoutDescription::from(p)
            })
            .collect();

        let table = tables::build::build_table(&projects_formatted);
        println!("{}", table);
    }

    println!("\n{}", pagination);
}
