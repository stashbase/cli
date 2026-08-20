use crate::{
    cmd::config::{AddProfile, ProfileSubcommand},
    config::{config, secure_store},
    handlers::config::api_key,
    models::config::Config,
    utils::output::{get_formatted_json_string, ColorizeIfColoredOutput},
};
use serde::Serialize;

pub fn handle_profile_command(command: ProfileSubcommand, config_data: &Config, json_output: bool) {
    match command {
        ProfileSubcommand::Add(args) => add_profile(args, config_data),
        ProfileSubcommand::List => list_profiles(config_data, json_output),
        ProfileSubcommand::Current => match config::resolve_profile_name(config_data) {
            Ok(profile) if json_output => print_json(&CurrentProfileOutput { profile }),
            Ok(profile) => println!("{}", profile.blue_bold_if_tty()),
            Err(error) => eprintln!("{} {}", "Error:".red_if_tty_stderr(), error),
        },
        ProfileSubcommand::Use(args) => match config::set_default_profile(&args.name) {
            Ok(()) => println!("Default profile set to '{}'.", args.name),
            Err(error) => eprintln!("{} {}", "Error:".red_if_tty_stderr(), error),
        },
        ProfileSubcommand::Remove(args) => {
            if let Err(error) = ensure_profile_can_be_removed(config_data, &args.name) {
                eprintln!("{} {}", "Error:".red_if_tty_stderr(), error);
                return;
            }
            if let Err(error) = secure_store::delete_api_key_for_profile(&args.name) {
                eprintln!(
                    "{} Profile '{}' was not removed because its secure-store key could not be deleted: {}",
                    "Error:".red_if_tty_stderr(),
                    args.name,
                    error
                );
                return;
            }
            match config::remove_profile(&args.name) {
                Ok(()) => println!("Profile '{}' removed.", args.name),
                Err(error) => eprintln!(
                    "{} The secure-store key was removed, but profile metadata could not be removed: {}",
                    "Warning:".yellow_if_tty_stderr(),
                    error
                ),
            }
        }
    }
}

fn ensure_profile_can_be_removed(config_data: &Config, name: &str) -> anyhow::Result<()> {
    config::validate_profile_name(name)?;
    if name == config::DEFAULT_PROFILE {
        anyhow::bail!(
            "The implicit 'default' profile cannot be removed. Use 'config api-key set' to replace its key."
        );
    }
    if !config_data
        .profiles
        .as_ref()
        .is_some_and(|profiles| profiles.contains_key(name))
    {
        anyhow::bail!("Profile '{name}' was not found.");
    }
    Ok(())
}

fn add_profile(args: AddProfile, config_data: &Config) {
    if let Err(error) = config::validate_profile_name(&args.name) {
        eprintln!("{} {}", "Error:".red_if_tty_stderr(), error);
        return;
    }
    if args.name == config::DEFAULT_PROFILE {
        eprintln!(
            "{} 'default' is the implicit profile; use 'config api-key set' to manage its key.",
            "Error:".red_if_tty_stderr()
        );
        return;
    }
    let api_key = match api_key::read_api_key(args.stdin) {
        Ok(key) => key,
        Err(error) => {
            eprintln!("{}", error.red_if_tty_stderr());
            return;
        }
    };
    if let Err(error) = secure_store::set_api_key_for_profile(&args.name, &api_key) {
        eprintln!(
            "{} Secure storage is required for profiles: {}",
            "Error:".red_if_tty_stderr(),
            error
        );
        return;
    }
    let profile_already_exists = config_data
        .profiles
        .as_ref()
        .is_some_and(|profiles| profiles.contains_key(&args.name));
    match config::add_profile(&args.name, args.workspace) {
        Ok(()) => println!("Profile '{}' saved.", args.name),
        Err(error) => {
            if !profile_already_exists {
                let _ = secure_store::delete_api_key_for_profile(&args.name);
            }
            eprintln!("{} {}", "Error:".red_if_tty_stderr(), error);
        }
    }
}

fn list_profiles(config_data: &Config, json_output: bool) {
    let default = config_data
        .default_profile
        .as_deref()
        .unwrap_or(config::DEFAULT_PROFILE);
    if json_output {
        let mut profiles = vec![ProfileListItem {
            name: config::DEFAULT_PROFILE.to_owned(),
            workspace: None,
            is_default: default == config::DEFAULT_PROFILE,
        }];
        if let Some(configured_profiles) = &config_data.profiles {
            profiles.extend(
                configured_profiles
                    .iter()
                    .filter(|(name, _)| name.as_str() != config::DEFAULT_PROFILE)
                    .map(|(name, profile)| ProfileListItem {
                        name: name.clone(),
                        workspace: profile.workspace.clone(),
                        is_default: name == default,
                    }),
            );
        }
        print_json(&ProfileListOutput { profiles });
        return;
    }
    print_profile_row(
        config::DEFAULT_PROFILE,
        None,
        default == config::DEFAULT_PROFILE,
    );
    if let Some(profiles) = &config_data.profiles {
        for (name, profile) in profiles {
            if name == config::DEFAULT_PROFILE {
                continue;
            }
            print_profile_row(name, profile.workspace.as_deref(), name == default);
        }
    }
}

#[derive(Serialize)]
struct CurrentProfileOutput {
    profile: String,
}

#[derive(Serialize)]
struct ProfileListOutput {
    profiles: Vec<ProfileListItem>,
}

#[derive(Serialize)]
struct ProfileListItem {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    workspace: Option<String>,
    is_default: bool,
}

fn print_json<T: Serialize>(value: &T) {
    match get_formatted_json_string(value, true) {
        Ok(json) => println!("{json}"),
        Err(error) => eprintln!("{} {}", "Error:".red_if_tty_stderr(), error),
    }
}

fn print_profile_row(name: &str, workspace: Option<&str>, is_default: bool) {
    let workspace = workspace
        .map(|workspace| format!(" ({workspace})").bright_black_if_tty())
        .unwrap_or_default();
    let default_marker = if is_default {
        " (default)".green_if_tty()
    } else {
        String::new()
    };
    println!("{}{}{}", name.blue_bold_if_tty(), workspace, default_marker);
}

#[cfg(test)]
mod tests {
    use super::{
        ensure_profile_can_be_removed, CurrentProfileOutput, ProfileListItem, ProfileListOutput,
    };
    use crate::{
        config::config::DEFAULT_PROFILE,
        models::config::{Config, ProfileConfig},
    };

    #[test]
    fn current_profile_json_has_a_stable_shape() {
        let output = CurrentProfileOutput {
            profile: "acme".to_owned(),
        };
        assert_eq!(
            serde_json::to_value(output).unwrap(),
            serde_json::json!({ "profile": "acme" })
        );
    }

    #[test]
    fn profile_list_json_omits_an_unset_workspace() {
        let output = ProfileListOutput {
            profiles: vec![ProfileListItem {
                name: "default".to_owned(),
                workspace: None,
                is_default: true,
            }],
        };
        assert_eq!(
            serde_json::to_value(output).unwrap(),
            serde_json::json!({
                "profiles": [{ "name": "default", "is_default": true }]
            })
        );
    }

    #[test]
    fn does_not_allow_removing_the_implicit_default_profile() {
        let error = ensure_profile_can_be_removed(&Config::new(), DEFAULT_PROFILE)
            .unwrap_err()
            .to_string();
        assert!(error.contains("cannot be removed"));
    }

    #[test]
    fn allows_removing_a_configured_named_profile() {
        let mut config = Config::new();
        config.profiles = Some(std::collections::BTreeMap::from([(
            "acme".to_owned(),
            ProfileConfig::default(),
        )]));
        assert!(ensure_profile_can_be_removed(&config, "acme").is_ok());
    }
}
