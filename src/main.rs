use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex,
};

use clap::Parser;
use cmd::root::Cli;
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
pub static REQUEST_TIMEOUT_SECS: OnceCell<u64> = OnceCell::new();
pub static REQUEST_ABORTED: AtomicBool = AtomicBool::new(false);

fn main() {
    init_logger();
    enable_virtual_terminal();
    set_handlers();

    let args = Cli::parse();
    set_color_choice(args.color);
    set_request_timeout_secs(args.timeout);

    handle_cli(args);

    if REQUEST_ABORTED.load(Ordering::SeqCst) {
        std::process::exit(130);
    }
}

#[cfg(windows)]
fn enable_virtual_terminal() {
    colored::control::set_virtual_terminal(true).unwrap();
}

#[cfg(not(windows))]
fn enable_virtual_terminal() {}

fn set_handlers() {
    // https://github.com/console-rs/dialoguer/issues/77
    ctrlc::set_handler(move || {
        if *SUBPROCESS_RUNNING.lock().unwrap() != true {
            let term = dialoguer::console::Term::stdout();
            let _ = term.show_cursor();

            eprintln!("");

            let already_aborted = REQUEST_ABORTED.swap(true, Ordering::SeqCst);
            if already_aborted {
                std::process::exit(130);
            }
        }
    })
    .expect("Error setting Ctrl-C handler");
}

fn set_color_choice(color_choice: ColorChoice) {
    COLOR_CHOICE.set(color_choice).unwrap();
}

fn set_request_timeout_secs(timeout_secs: Option<u64>) {
    let timeout = timeout_secs.unwrap_or(30);
    REQUEST_TIMEOUT_SECS.set(timeout).unwrap();
}
