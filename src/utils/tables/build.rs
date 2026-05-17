use tabled::{
    settings::{object::Rows, peaker::PriorityMax, Color, Modify, Settings, Style, Width},
    Table, Tabled,
};

use crate::utils::{output::is_color_enabled, term_size::get_terminal_size};

pub fn build_table<T>(secrets: &Vec<T>) -> Table
where
    T: Tabled,
{
    let (width, _) = get_terminal_size();
    let mut table = Table::new(secrets);

    if is_color_enabled(true) {
        let term_size_settings = Settings::default()
            // .with(Style::modern())
            .with(Style::blank())
            .with(Width::wrap(width).priority::<PriorityMax>())
            .with(Modify::new(Rows::first()).with(Color::BOLD | Color::FG_BLUE));

        table.with(term_size_settings);
    } else {
        let term_size_settings = Settings::default()
            // .with(Style::modern())
            .with(Style::blank())
            .with(Width::wrap(width).priority::<PriorityMax>());

        table.with(term_size_settings);
    }

    table
}
