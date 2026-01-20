use crate::{
    cmd::shared::Scope, config::config, models::config::UpdateConfig,
    utils::output::ColorizeIfColoredOutput,
};

pub fn set_scope(scope: Scope) {
    let res = config::update_config(UpdateConfig {
        scope: Some(scope),
        api_key: None,
        expand_refs: None,
        output_format: None,
    });

    if let Err(err) = res {
        eprintln!("{} {}", "Error:".red_if_tty_stderr(), err);
    } else {
        let msg = format!("{} {}", "✔".green_if_tty(), "Scope set.");
        println!("{}", msg);
    }
}

pub fn print_scope(scope: &Scope) {
    println!("Scope: {}.", scope);
}
