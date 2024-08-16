use anyhow::{bail, Result};
use colored_json::to_colored_json_auto;
use log::{debug, error};

use crate::{
    api::projects,
    cmd::{config::OutputFormat, projects::Sort},
    models::{
        api_client::GetRequestApiResponse,
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
    pub sort: Option<Sort>,
    pub descending: bool,
    pub page: Option<usize>,
    pub limit: Option<usize>,
    pub format: OutputFormat,
}

pub async fn handle_list_projects(args: HandleListProjectsArgs) -> Result<()> {
    let HandleListProjectsArgs {
        api_key,
        search,
        sort,
        descending,
        format,
        page,
        limit,
    } = args;

    // validate search
    if let Some(search) = &search {
        let search_validation_res = validate_project_search(&search);

        if let Err(err) = search_validation_res {
            bail!(err);
        }
    }

    if let Some(limit) = limit {
        if limit < 2 || limit > 30 {
            let error = InputValidationError::Projects(ProjectInputValidationError::InvalidLimit);
            bail!(error);
        }
    }

    debug!("listing projects...:");

    let mut spinner = request_spinner();
    let project_res = projects::list_projects(
        api_key,
        search,
        sort.unwrap_or(Sort::Created),
        descending,
        page,
        limit,
    )
    .await;

    if let Err(err) = project_res {
        spinner.stop_and_persist("", "");
        error!("{:#?}", &err);
        bail!(err);
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
                        output_json(&data);
                        return Ok(());
                    }

                    let mut projects = data.data;
                    let pagination = data.pagination;

                    if projects.is_empty() {
                        spinner.stop_with_message("No projects found");
                        eprintln!("\n{}", pagination);
                    } else {
                        spinner.stop_and_persist("", "");

                        match format {
                            OutputFormat::List => {
                                output_list(projects, pagination);
                            }
                            OutputFormat::Table => {
                                // reverse because returned fro list -> last is first (for
                                // lists)
                                projects.reverse();
                                output_table(projects, pagination);
                            }
                            OutputFormat::Json => unreachable!(),
                        }
                    }
                }
                Err(e) => {
                    debug!("{:#?}", &e);
                    spinner.stop_and_persist("", "");
                    bail!("Something went wrong")
                }
            }
        }
        GetRequestApiResponse::Err(e) => {
            spinner.stop_with_message(&format!("{}", e));
            // spinner.stop_and_persist("", "");
            // error!("{:#?}", &e);
            // eprint!("{}", e);
        }
    }

    Ok(())
    //
    // let project_res = projects::list_projects(api_key).await;
    // spinner.stop_and_persist("", "");
    //
    // if let Err(err) = &project_res {
    //     error!("{:#?}", &err);
    //     bail!("Could not connect to API")
    // }
    //
    // let project_res = project_res.unwrap();
    //
    // let status = project_res.status();
    //
    // if status == 401 {
    //     bail!("Unauthorized")
    // }
    //
    // let response_text = project_res.text().await;
    // debug!("{:#?}", &response_text);
    //
    // match response_text {
    //     Ok(text) => {
    //         let projects = serde_json::from_str::<Vec<Project>>(&text);
    //
    //         match projects {
    //             Ok(projects) => {
    //                 debug!("{:#?}", &projects);
    //                 let value = serde_json::to_value(&projects).unwrap();
    //                 let pretty = to_colored_json_auto(&value).unwrap();
    //
    //                 println!("{}", pretty);
    //             }
    //             Err(e) => {
    //                 error!("{:#?}", &e);
    //                 bail!("Something went wrong")
    //             }
    //         }
    //     }
    //     Err(err) => {
    //         bail!("Could not parse response: {:?}", err);
    //     }
    // }
    //
    // Ok(())
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
