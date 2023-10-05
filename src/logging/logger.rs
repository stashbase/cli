use env_logger::Builder;
use log::LevelFilter;
use owo_colors::OwoColorize;
use std::io::Write;

pub fn init_logger() {
    Builder::new()
        .format(|buf, record| {
            let msg = match record.level() {
                log::Level::Error => "ERROR".to_string().red().bold().to_string(),
                log::Level::Warn => "WARN".to_string().yellow().bold().to_string(),
                log::Level::Info => "INFO".to_string().green().bold().to_string(),
                log::Level::Debug => "DEBUG".to_string().bright_yellow().bold().to_string(),
                log::Level::Trace => "TRACE".to_string().magenta().bold().to_string(),
            };

            writeln!(
                buf,
                "[{}] {}: [{}] {}",
                record.file().unwrap_or("unknown"),
                record.line().unwrap_or(0),
                msg,
                record.args()
            )
        })
        .filter(None, LevelFilter::Debug)
        .init()
}
