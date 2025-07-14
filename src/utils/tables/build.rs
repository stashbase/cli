use tabled::{
    settings::{object::Rows, peaker::PriorityMax, Color, Modify, Settings, Style, Width},
    Table, Tabled,
};

use crate::utils::{output::is_terminal, term_size::get_terminal_size};

pub fn build_table<T>(secrets: &Vec<T>) -> Table
where
    T: Tabled,
{
    let (width, _) = get_terminal_size();

    if is_terminal() {
        let term_size_settings = Settings::default()
            // .with(Style::modern())
            .with(Style::rounded())
            .with(Width::wrap(width).priority::<PriorityMax>())
            .with(Modify::new(Rows::first()).with(Color::FG_GREEN));

        let mut table = Table::new(secrets);
        table.with(term_size_settings);

        table
    } else {
        let term_size_settings = Settings::default()
            // .with(Style::modern())
            .with(Style::rounded())
            .with(Width::wrap(width).priority::<PriorityMax>());

        let mut table = Table::new(secrets);
        table.with(term_size_settings);

        table
    }
}
