use anyhow::{bail, Result};
use owo_colors::OwoColorize;

use crate::{
    cmd::state::{StateCommand, StateSubcommand},
    models::{
        config::{Config, State},
        validation::{CmdArgInputValidationError, InputValidationError},
    },
    utils::validation::{validate_environment_name, validate_project_name},
};

use crate::{config::config, models::config::UpdateConfig};

pub fn handle_state_commands(cmd: StateCommand, config: &mut Config) -> Result<()> {
    match cmd.subcommand {
        StateSubcommand::Set(cmd) => {
            if cmd.project.is_none() && cmd.environment.is_none() {
                bail!(InputValidationError::CmdArgs(
                    CmdArgInputValidationError::MissingProjectOrEnvironment
                ))
            }

            if let Some(project) = &cmd.project {
                validate_project_name(&project, false, false)?;
            }
            if let Some(environment) = &cmd.environment {
                validate_environment_name(&environment, false, false)?;
            }

            handle_set_state(cmd.project, cmd.environment);
        }
        StateSubcommand::Print => {
            if let Some(state) = &config.state {
                if state.is_empty() {
                    eprintln!("No state set");
                } else {
                    print!("{state}")
                }
            } else {
                eprintln!("No state set");
            }
        }
        StateSubcommand::Unset(cmd) => {
            if cmd.project == false && cmd.environment == false {
                bail!(InputValidationError::CmdArgs(
                    CmdArgInputValidationError::MissingProjectOrEnvironment
                ))
            }

            if let Some(state) = &config.state {
                handle_unset_state(
                    config,
                    state.project.clone(),
                    state.environment.clone(),
                    cmd.project,
                    cmd.environment,
                );
            } else {
                eprintln!("{}", "No state to unset");
            }
        }
    }

    Ok(())
}

fn handle_set_state(project: Option<String>, environment: Option<String>) {
    let state = State {
        project,
        environment,
    };

    let res = config::update_config(UpdateConfig {
        state: Some(state),
        api_key: None,
        output_format: None,
    });

    if let Err(err) = res {
        eprintln!("{} {}", "Error:".red(), err);
    } else {
        let msg = format!("{} {}", "✔".green(), "State has been set");
        eprintln!("{}", msg);
    }
}

fn handle_unset_state(
    config: &mut Config,
    state_project: Option<String>,
    state_environment: Option<String>,
    unset_project: bool,
    unset_environment: bool,
) {
    let updated_state = State {
        project: if unset_project { None } else { state_project },
        environment: if unset_environment {
            None
        } else {
            state_environment
        },
    };

    let res = config::update_config_state(config, Some(updated_state));

    if let Err(err) = res {
        eprintln!("{} {}", "Error:".red(), err);
    } else {
        let msg = format!("{} {}", "✔".green(), "State has been updated");
        eprintln!("{}", msg);
    }
}
