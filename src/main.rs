use clap::Parser;
use cmd::root::Cli;
use log::debug;
use logging::logger::init_logger;

use crate::handlers::entry::handle_cli;

mod cmd;
mod handlers;
mod logging;

fn main() {
    init_logger();

    let args = Cli::parse();
    debug!("Args: {:?}", args);

    handle_cli(args);
}
