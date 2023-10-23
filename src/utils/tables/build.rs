use tabled::{
    settings::{object::Rows, peaker::PriorityMax, Color, Modify, Settings, Style, Width},
    Table,
};

use crate::{models::secrets::SecretWithoutDescription, utils::term_size::get_terminal_size};

pub fn build_secrets_table(secrets: &Vec<SecretWithoutDescription>) -> Table {
    let (width, _) = get_terminal_size();

    let term_size_settings = Settings::default()
        // .with(Style::modern())
        .with(Style::rounded())
        .with(Width::wrap(width).priority::<PriorityMax>())
        .with(Modify::new(Rows::first()).with(Color::FG_GREEN));
    // .with(Width::increase(width))
    // .with(Height::limit(height))
    // .with(Height::increase(height));
    // .with(Width::list([width / 3, width / 3]));

    let mut table = Table::new(secrets);
    table.with(term_size_settings);

    table
}
