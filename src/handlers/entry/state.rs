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

pub fn handle_state_commands(cmd: StateCommand, config: &Config) -> Result<()> {
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
                let project = &state.project;
                let environment = &state.environment;

                if project.is_none() && environment.is_none() {
                    eprintln!("{}", "No state set");
                } else {
                    if let Some(project) = project {
                        println!("Project: {}", *project);
                    }

                    if let Some(environment) = environment {
                        println!("Environment: {}", *environment);
                    }
                }
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
