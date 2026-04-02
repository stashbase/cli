use std::{collections::HashSet, path::Path};

use crate::{
    api::secrets,
    cmd::{pull::PullFormat, push::PushFormat, secrets::SecretsFileFormat},
    handlers::run::entry::{
        get_set_name_comment_pairs, get_set_name_value_pairs, validate_no_duplicate_set_names,
    },
    models::{
        api_client::RequestApiOptionResponse,
        config_env::{ConfigActionCommand, EnvConfigItem},
        scope::Scope,
        secrets::{FormatSecrets, Secret, ValidateSecrets},
        validation::{
            InputValidationError, LoadEnvironmentInputValidationError,
            PushPullInputValidationError, SecretsInputValidationError,
        },
    },
    utils::{
        self, interaction,
        output::ColorizeIfColoredOutput,
        secrets::read_secrets_from_file,
        validation::{
            map_secret_to_load_exclude_secrets_error, map_secret_to_load_only_secrets_error,
            validate_project_environment_identifier, validate_secret_names,
        },
    },
};
use anyhow::{bail, Result};
use log::debug;
use spinoff::{spinners, Color, Spinner, Streams};

#[derive(Debug)]
pub struct HandlePushArgs {
    pub api_key: String,
    pub scope: Option<Scope>,

    pub config_file_path: Option<String>,
    //
    pub target_file: Option<String>,
    pub format: Option<PushFormat>,
    //
    pub only: Vec<String>,
    pub set: Vec<String>,
    pub set_comments: Vec<String>,
    pub exclude: Vec<String>,
    pub expand_refs: Option<bool>,
    pub ignore_comments: Option<bool>,
    pub json_format: bool,
    pub silent: bool,
}

pub async fn handle_push(args: HandlePushArgs) -> Result<()> {
    let HandlePushArgs {
        api_key,
        scope,
        config_file_path,
        only,
        exclude,
        mut expand_refs,
        mut set,
        set_comments,
        mut format,
        mut target_file,
        mut ignore_comments,
        json_format,
        silent,
    } = args;

    if let Err(error) = validate_no_duplicate_set_names(&set) {
        let error_output = error.format_error_output(json_format)?;

        if !silent {
            eprintln!();
        }
        bail!(error_output);
    }

    // Handle environment scope - workspace scope behaves like no scope
    let is_environment_scope = scope.as_ref() == Some(&Scope::Environment);

    let config_action_command = ConfigActionCommand::Push;

    let project: Option<String>;
    let environment: Option<String>;

    let mut only_set: HashSet<_> = only.into_iter().collect();
    let mut exclude_set: HashSet<_> = exclude.into_iter().collect();
    let mut config_set_comments = std::collections::HashMap::<String, String>::new();

    // Handle environment scope differently - skip config file loading
    if is_environment_scope {
        // For environment scope, we don't need config file
        project = None;
        environment = None;
    } else {
        let selected_config_item =
            EnvConfigItem::select_from_file(config_file_path.clone(), &config_action_command)?;
        if let Some(config) = selected_config_item {
            if let None = target_file {
                let target_file_path = config.get_push_target_file();
                target_file = target_file_path;
            }

            if let None = target_file {
                let err = InputValidationError::PushPullEnvironment(
                    PushPullInputValidationError::NoFileSpecified { is_push: true },
                );
                let error_output = err.format_error_output(json_format)?;

                if !silent {
                    eprintln!();
                }
                bail!(error_output);
            }

            if let None = format {
                let format_config = config.get_push_format();
                format = format_config;
            }

            let secrets_config = config.get_push_secrets();

            // expand refs
            if let Some(expand_refs_val) = secrets_config.expand_refs {
                if expand_refs.is_none() {
                    expand_refs = Some(expand_refs_val);
                }
            }

            if let Some(ignore_comments_val) = secrets_config.ignore_comments {
                if ignore_comments.is_none() {
                    ignore_comments = Some(ignore_comments_val);
                }
            }

            if let Some(only_secrets_config) = secrets_config.only {
                for only_secret in only_secrets_config {
                    only_set.insert(only_secret);
                }
            }

            if let Some(exclude_secrets_config) = secrets_config.exclude {
                for exclude_secret in exclude_secrets_config {
                    exclude_set.insert(exclude_secret);
                }
            }

            // set
            if let Some(set_val) = secrets_config.set {
                if set_val.is_empty() == false {
                    let mut set_secrets_from_file = Vec::new();
                    let mut seen_set_names = HashSet::new();
                    let mut duplicate_set_names = Vec::new();
                    let mut duplicate_set_seen = HashSet::new();

                    for item in set_val {
                        if !seen_set_names.insert(item.name.clone())
                            && duplicate_set_seen.insert(item.name.clone())
                        {
                            duplicate_set_names.push(item.name.clone());
                        }

                        if ignore_comments != Some(true) {
                            if let Some(comment) = item.comment {
                                config_set_comments.insert(item.name.clone(), comment);
                            }
                        }

                        let name_value_str = format!("{}={}", item.name, item.value);

                        if set.contains(&name_value_str) == false {
                            set_secrets_from_file.push(name_value_str);
                        }
                    }

                    if !duplicate_set_names.is_empty() {
                        let error = InputValidationError::LoadEnvironment(
                            LoadEnvironmentInputValidationError::SetDuplicateNames(
                                duplicate_set_names,
                            ),
                        );
                        let error_output = error.format_error_output(json_format)?;

                        if !silent {
                            eprintln!();
                        }
                        bail!(error_output);
                    }

                    set = [set_secrets_from_file, set].concat();
                }
            }

            project = Some(config.project);
            environment = Some(config.environment);
        } else {
            return Ok(());
        }
    }

    let should_ignore_comments = ignore_comments == Some(true);

    let input_path = target_file.unwrap();
    let path = Path::new(&input_path);

    let file_exists = path.exists();

    if !file_exists {
        let err = InputValidationError::Secrets(SecretsInputValidationError::FileNotFound);
        let error_output = err.format_error_output(json_format)?;

        if !silent {
            eprintln!();
        }

        bail!(error_output);
    }

    //
    let target_format = match format {
        Some(f) => match f {
            PullFormat::Dotenv => SecretsFileFormat::Dotenv,
            PullFormat::Yaml => SecretsFileFormat::Yaml,
            PullFormat::Json => SecretsFileFormat::Json,
        },
        None => {
            if input_path.ends_with(".yaml") || input_path.ends_with(".yml") {
                SecretsFileFormat::Yaml
            } else if input_path.ends_with(".json") {
                SecretsFileFormat::Json
            } else {
                SecretsFileFormat::Dotenv
            }
        }
    };

    let secrets_res = read_secrets_from_file(path, &target_format);

    if let Err(err) = secrets_res {
        let err =
            InputValidationError::Secrets(SecretsInputValidationError::ReadFile(err.to_string()));

        let error_output = err.format_error_output(json_format)?;

        if !silent {
            eprintln!();
        }
        bail!(error_output);
    }

    // Validation logic - skip for environment scope
    if !is_environment_scope {
        let project_ref = project.as_ref().unwrap();
        let environment_ref = environment.as_ref().unwrap();

        let validation_res =
            validate_project_environment_identifier(project_ref, environment_ref, true);

        if let Err(e) = validation_res {
            let error_output = e.format_error_output(json_format)?;

            if !silent {
                eprintln!();
            }
            bail!(error_output);
        }
    }

    //  process, format and validate secrets
    let mut secrets = secrets_res.unwrap();

    if let Some(ignore_comments) = ignore_comments {
        if ignore_comments == true {
            secrets = secrets
                .into_iter()
                .map(|secret| secret.without_comment())
                .collect();
        }
    }

    if !only_set.is_empty() && !exclude_set.is_empty() {
        let err = InputValidationError::LoadEnvironment(
            LoadEnvironmentInputValidationError::UseOfBothExcludeAndOnly,
        );

        let error_output = err.format_error_output(json_format)?;

        if !silent {
            eprintln!();
        }
        bail!(error_output);
    }

    if only_set.is_empty() == false {
        // filter unwanted secrets
        let names_vec = only_set.iter().cloned().collect::<Vec<_>>();
        let name_validation_res = validate_secret_names(&names_vec);

        if let Err(err) = name_validation_res {
            let mapped_err = map_secret_to_load_only_secrets_error(&err);
            let error_output = InputValidationError::LoadEnvironment(mapped_err)
                .format_error_output(json_format)?;

            if !silent {
                eprintln!();
            }
            bail!(error_output);
        }

        secrets = secrets
            .into_iter()
            .filter(|secret| only_set.contains(&secret.name))
            .collect::<Vec<_>>();
    } else if !exclude_set.is_empty() {
        let names_vec = exclude_set.iter().cloned().collect::<Vec<_>>();
        let name_validation_res = validate_secret_names(&names_vec);

        if let Err(err) = name_validation_res {
            let mapped_err = map_secret_to_load_exclude_secrets_error(&err);
            let error_output = InputValidationError::LoadEnvironment(mapped_err)
                .format_error_output(json_format)?;

            if !silent {
                eprintln!();
            }
            bail!(error_output);
        }

        secrets = secrets
            .into_iter()
            .filter(|secret| !exclude_set.contains(&secret.name))
            .collect();
    }

    if secrets.is_empty() {
        if !silent {
            if json_format {
                let message = serde_json::json!({
                    "message": "Nothing to upload: no secrets found."
                });

                eprintln!("{}", message);
            } else {
                let msg = format!(
                    "{}: {}",
                    "Nothing to upload".yellow_if_tty_stderr(),
                    "no secrets found."
                );
                eprintln!("{}", msg);
            }
        }

        return Ok(());
    }

    if !set.is_empty() {
        let name_values_pairs = get_set_name_value_pairs(set);

        match name_values_pairs {
            Ok(set_secrets) => {
                let set_secret_names: HashSet<String> =
                    set_secrets.iter().map(|(name, _)| name.clone()).collect();

                if !should_ignore_comments && !set_comments.is_empty() {
                    let comments_pairs = get_set_name_comment_pairs(set_comments);

                    match comments_pairs {
                        Ok(comments) => {
                            let mut missing_set_names = Vec::<String>::new();

                            for (name, comment) in comments {
                                if set_secret_names.contains(&name) {
                                    config_set_comments.insert(name, comment);
                                } else {
                                    missing_set_names.push(name);
                                }
                            }

                            if !missing_set_names.is_empty() {
                                let error = InputValidationError::LoadEnvironment(
                                    LoadEnvironmentInputValidationError::SetCommentWithoutSet(
                                        missing_set_names,
                                    ),
                                );
                                let error_output = error.format_error_output(json_format)?;

                                if !silent {
                                    eprintln!();
                                }
                                bail!(error_output);
                            }
                        }
                        Err(e) => {
                            let error_output = e.format_error_output(json_format)?;

                            if !silent {
                                eprintln!();
                            }
                            bail!(error_output);
                        }
                    }
                }

                for (name, value) in set_secrets {
                    let config_comment = if should_ignore_comments {
                        None
                    } else {
                        config_set_comments.get(&name).cloned()
                    };
                    // find index
                    let index = secrets.iter().position(|secret| secret.name == name);

                    if let Some(index) = index {
                        let existing_secret = &mut secrets[index];
                        existing_secret.value = value;
                        if config_comment.is_some() {
                            existing_secret.comment = config_comment;
                        }
                    } else {
                        let new_secret = Secret {
                            name,
                            value,
                            comment: config_comment,
                        };

                        secrets.push(new_secret);
                    }
                }
            }
            Err(e) => {
                if !silent {
                    eprintln!();
                }
                bail!(e);
            }
        }
    } else if !should_ignore_comments && !set_comments.is_empty() {
        let comments_pairs = get_set_name_comment_pairs(set_comments);

        match comments_pairs {
            Ok(comments) => {
                let missing_set_names = comments
                    .into_iter()
                    .map(|(name, _)| name)
                    .collect::<Vec<String>>();
                let error = InputValidationError::LoadEnvironment(
                    LoadEnvironmentInputValidationError::SetCommentWithoutSet(
                        missing_set_names,
                    ),
                );
                let error_output = error.format_error_output(json_format)?;

                if !silent {
                    eprintln!();
                }
                bail!(error_output);
            }
            Err(e) => {
                let error_output = e.format_error_output(json_format)?;

                if !silent {
                    eprintln!();
                }
                bail!(error_output);
            }
        }
    }

    // format secrets for input
    secrets.format();

    // validate secrets
    if let Err(err) = secrets.validate() {
        if !silent {
            eprintln!();
        }
        bail!(err);
    }

    let reference_warnings = secrets.get_reference_warnings();

    if !reference_warnings.is_empty() && !silent {
        eprint!("{}", reference_warnings);
    }

    if !silent {
        let info = format!("Number of secrets to push: {}", secrets.len());
        eprintln!("{}", info);
    }

    let confirm = if !silent {
        interaction::confirm_opt("Are you sure you want to continue?")
    } else {
        Some(true) // Auto-proceed in silent mode
    };

    if confirm.is_none() || (confirm.unwrap() == false) {
        return Ok(());
    }

    if !silent {
        eprintln!();
    }

    if let Some(expand_refs) = expand_refs {
        if expand_refs == true {
            utils::secrets::expand_secret_references(&mut secrets);
        }
    }

    let mut spinner = if !silent {
        Some(Spinner::new_with_stream(
            spinners::Dots,
            "Pushing secrets...",
            Color::Cyan,
            Streams::Stderr,
        ))
    } else {
        None
    };

    // Determine project and environment for API call
    let (api_project, api_environment) = if is_environment_scope {
        // For environment scope, pass None (relies on environment-scoped API key)
        (None, None)
    } else {
        (project.clone(), environment.clone())
    };

    // file
    let res = secrets::set_sercrets(api_key, api_project, api_environment, &secrets).await;

    if let Err(err) = res {
        if let Some(ref mut spinner) = spinner {
            spinner.stop_and_persist("", "");
        }
        debug!("Error: {:#?}", &err);

        let error_output = err.format_error_output(json_format)?;
        bail!(error_output);
    }

    let res = res.unwrap();

    match res {
        RequestApiOptionResponse::Ok(_) => {
            if json_format {
                if let Some(ref mut spinner) = spinner {
                    spinner.stop_and_persist("", "");
                }
                println!("{{}}");
            } else {
                if let Some(ref mut spinner) = spinner {
                    spinner.stop_with_message("Secrets pushed.");
                } else if !silent {
                    println!("Secrets pushed.");
                }
            }
        }
        RequestApiOptionResponse::Err(e) => {
            debug!("Error: {}", e);
            if let Some(ref mut spinner) = spinner {
                spinner.stop_and_persist("", "");
            }

            let error_output = e.format_error_output(json_format)?;
            bail!(error_output);
        }
    }

    Ok(())
}
