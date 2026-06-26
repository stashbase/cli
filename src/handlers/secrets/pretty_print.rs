pub const SECRET_NAME_PREVIEW_LIMIT: usize = 5;

pub fn print_secret_name_list(title: &str, names: &[String]) {
    if names.is_empty() {
        return;
    }

    println!();
    println!("{}", title);

    for name in names.iter().take(SECRET_NAME_PREVIEW_LIMIT) {
        println!("- {}", name);
    }

    let remaining = names.len().saturating_sub(SECRET_NAME_PREVIEW_LIMIT);
    if remaining > 0 {
        println!("...and {} more", remaining);
    }
}
