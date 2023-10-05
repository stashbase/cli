use clap::Parser;
use cmd::root::Cli;
use log::debug;
use logging::logger::init_logger;

mod cmd;
mod logging;

fn main() {
    init_logger();

    let args = Cli::parse();

    debug!("Args: {:?}", args);
}
