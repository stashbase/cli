// use anyhow::Result;
use owo_colors::OwoColorize;

use crate::{config::config, models::config::UpdateConfig};

pub fn set_api_key(api_key: String) {
    let res = config::update_config(UpdateConfig {
        api_key: Some(api_key),
    });

    if let Err(err) = res {
        eprintln!("{} {}", "Error:".red(), err);
    } else {
        let msg = format!("{} {}", "✔".green(), "API Key has been set");
        eprintln!("{}", msg);
    }
}
