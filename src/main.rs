use clap::Parser;
use cmd::root::Cli;
use log::debug;
use logging::logger::init_logger;

use crate::handlers::entry::root::handle_cli;

mod api;
mod cmd;
mod config;
mod handlers;
mod logging;
mod models;
mod utils;

fn main() {
    init_logger();
    set_handlers();

    let args = Cli::parse();
    debug!("Args: {:?}", args);

    handle_cli(args);
}

fn set_handlers() {
    // https://github.com/console-rs/dialoguer/issues/77
    ctrlc::set_handler(move || {
        let term = dialoguer::console::Term::stdout();
        let _ = term.show_cursor();

        eprintln!("");
        // exit process
        std::process::exit(0);
    })
    .expect("Error setting Ctrl-C handler");
}
