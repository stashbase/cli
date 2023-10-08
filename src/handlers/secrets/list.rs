use anyhow::{bail, Result};
use colored_json::to_colored_json_auto;
use log::debug;

use crate::{
    api::secrets,
    cmd::secrets::SecretsFromat,
    models::{api_client::GetRequestApiResponse, secrets::Secret},
    utils::{secrets::format_secrets, spinner::request_spinner, validation::validate_project_name},
};

pub struct HandleListSecretsArgs {
    pub token: String,
    pub project: String,
    pub environment: String,
    pub format: SecretsFromat,
}

pub async fn handle_list_secrets(args: HandleListSecretsArgs) -> Result<()> {
    let HandleListSecretsArgs {
        token,
        project,
        environment: enironment,
        format,
    } = args;

    // TODO: other validations
    let name_is_valid = validate_project_name(&project, false);

    if let Err(err) = name_is_valid {
        bail!(err);
    }

    debug!("listing secrets...:");

    let mut spinner = request_spinner();
    let res = secrets::list(token, project, enironment).await;

    spinner.stop_and_persist("", "");

    if let Err(err) = res {
        debug!("Error: {:#?}", &err);
        bail!(err);
    }

    let res = res.unwrap();

    match res {
        GetRequestApiResponse::Ok(data) => {
            let secrets = serde_json::from_str::<Vec<Secret>>(&data.text);

            match secrets {
                Ok(secrets) => {
                    debug!("{:#?}", &secrets);

                    let print_string = format_secrets(secrets, &format);

                    if format == SecretsFromat::List {
                        print!("{}", print_string);
                    } else {
                        println!("{}", print_string);
                    }

                    // if format == Some(SecretsFromat::Dotenv) {
                    //     let dotenv_string: String = secrets
                    //         .iter()
                    //         .enumerate()
                    //         .map(|(i, s)| {
                    //             if let Some(descr) = &s.description {
                    //                 if i != secrets.len() - 1 {
                    //                     return format!("# {}\n{}={}\n", descr, s.key, s.value);
                    //                 } else {
                    //                     return format!("# {}\n{}={}", descr, s.key, s.value);
                    //                 }
                    //             } else {
                    //                 if i != secrets.len() - 1 {
                    //                     return format!("{}={}\n", s.key, s.value);
                    //                 } else {
                    //                     return format!("{}={}", s.key, s.value);
                    //                 }
                    //             }
                    //         })
                    //         .collect::<_>();
                    //
                    //     println!("{}", dotenv_string);
                    // } else if raw || format == Some(SecretsFromat::Json) {
                    //     let value = serde_json::to_value(&secrets).unwrap();
                    //     let pretty = to_colored_json_auto(&value).unwrap();
                    //
                    //     println!("{}", pretty);
                    // } else {
                    //     for (i, p) in secrets.iter().enumerate() {
                    //         if i == secrets.len() - 1 {
                    //             print!("{}", p);
                    //         } else {
                    //             println!("{}", p);
                    //         }
                    //     }
                    // }
                }
                Err(_) => {
                    bail!("Something went wrong")
                }
            }
        }
        GetRequestApiResponse::Err(e) => {
            bail!("{}", e);
        }
    }

    Ok(())
}
