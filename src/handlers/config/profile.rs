use crate::{
    cmd::config::{AddProfile, ProfileSubcommand},
    config::{config, secure_store},
    handlers::config::api_key,
    models::config::Config,
    utils::output::ColorizeIfColoredOutput,
};

pub fn handle_profile_command(
    command: ProfileSubcommand,
    config_data: &Config,
    cli_profile: Option<&str>,
) {
    match command {
        ProfileSubcommand::Add(args) => add_profile(args),
        ProfileSubcommand::List => list_profiles(config_data),
        ProfileSubcommand::Current => {
            match config::resolve_profile_name(config_data, cli_profile) {
                Ok(profile) => println!("{profile}"),
                Err(error) => eprintln!("{} {}", "Error:".red_if_tty_stderr(), error),
            }
        }
        ProfileSubcommand::Use(args) => match config::set_default_profile(&args.name) {
            Ok(()) => println!("Default profile set to '{}'.", args.name),
            Err(error) => eprintln!("{} {}", "Error:".red_if_tty_stderr(), error),
        },
        ProfileSubcommand::Remove(args) => match config::remove_profile(&args.name) {
            Ok(()) => {
                if let Err(error) = secure_store::delete_api_key_for_profile(&args.name) {
                    eprintln!(
                        "{} Profile removed, but its secure-store key could not be deleted: {}",
                        "Warning:".yellow_if_tty_stderr(),
                        error
                    );
                } else {
                    println!("Profile '{}' removed.", args.name);
                }
            }
            Err(error) => eprintln!("{} {}", "Error:".red_if_tty_stderr(), error),
        },
    }
}

fn add_profile(args: AddProfile) {
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
    match config::add_profile(&args.name, args.workspace) {
        Ok(()) => println!("Profile '{}' saved.", args.name),
        Err(error) => {
            let _ = secure_store::delete_api_key_for_profile(&args.name);
            eprintln!("{} {}", "Error:".red_if_tty_stderr(), error);
        }
    }
}

fn list_profiles(config_data: &Config) {
    let default = config_data
        .default_profile
        .as_deref()
        .unwrap_or(config::DEFAULT_PROFILE);
    println!(
        "{}{}",
        config::DEFAULT_PROFILE,
        if default == config::DEFAULT_PROFILE {
            " (default)"
        } else {
            ""
        }
    );
    if let Some(profiles) = &config_data.profiles {
        for (name, profile) in profiles {
            if name == config::DEFAULT_PROFILE {
                continue;
            }
            let workspace = profile
                .workspace
                .as_deref()
                .map(|value| format!(" ({value})"))
                .unwrap_or_default();
            let marker = if name == default { " (default)" } else { "" };
            println!("{name}{workspace}{marker}");
        }
    }
}
