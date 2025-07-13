use crate::{
    config::config::{create_config, get_config_path},
    utils::interaction,
};
use anyhow::Result;

pub fn reset_config(force: bool) -> Result<()> {
    if !force {
        eprintln!("\nContent of the config file will be lost.");
        let i = interaction::confirm_opt("Are you sure?");

        if i.is_none() || (i.unwrap() == false) {
            return Ok(());
        }
    }

    let config_path = get_config_path()?;
    create_config(config_path.as_path())?;

    Ok(())
}
