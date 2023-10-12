// use anyhow::Result;
use owo_colors::OwoColorize;

use crate::{config::config, models::config::UpdateConfig};

pub fn set_token(token: String) {
    let res = config::update_config(UpdateConfig { token: Some(token) });

    if let Err(err) = res {
        eprintln!("{} {}", "Error:".red(), err);
    } else {
        let msg = format!("{} {}", "✔".green(), "Token has been set");
        eprintln!("{}", msg);
    }
}
