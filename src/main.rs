use clap::Parser;
use cmd::root::Cli;
use log::debug;
use logging::logger::init_logger;

use crate::handlers::entry::handle_cli;

mod api;
mod cmd;
mod config;
mod handlers;
mod logging;
mod models;
mod utils;

fn main() {
    init_logger();

    let args = Cli::parse();
    debug!("Args: {:?}", args);

    handle_cli(args);
}
