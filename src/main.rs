use std::sync::Mutex;

use clap::Parser;
use cmd::root::Cli;
use log::debug;
use logging::logger::init_logger;
use once_cell::sync::{Lazy, OnceCell};

use crate::{cmd::root::ColorChoice, handlers::entry::root::handle_cli};

mod api;
mod cmd;
mod config;
mod handlers;
mod logging;
mod models;
mod utils;

pub static SUBPROCESS_RUNNING: Lazy<Mutex<bool>> = Lazy::new(|| Mutex::new(false));
pub static COLOR_CHOICE: OnceCell<ColorChoice> = OnceCell::new();

fn main() {
    init_logger();
    set_handlers();

    let args = Cli::parse();
    debug!("Args: {:?}", args);

    set_color_choice(args.color);

    handle_cli(args);
}

fn set_handlers() {
    // https://github.com/console-rs/dialoguer/issues/77
    ctrlc::set_handler(move || {
        if *SUBPROCESS_RUNNING.lock().unwrap() != true {
            let term = dialoguer::console::Term::stdout();
            let _ = term.show_cursor();

            eprintln!("");

            // exit process
            std::process::exit(0);
        }
    })
    .expect("Error setting Ctrl-C handler");
}

fn set_color_choice(color_choice: ColorChoice) {
    COLOR_CHOICE.set(color_choice).unwrap();
}
