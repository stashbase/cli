/// Removes surrounding single or double quotes from a string if present,
/// otherwise returns the string unchanged.
pub fn format_env_variable_value(input: String) -> String {
    if input.len() >= 2 {
        match (input.chars().next(), input.chars().last()) {
            (Some('"'), Some('"')) => input[1..input.len() - 1].to_string(),
            (Some('\''), Some('\'')) => input[1..input.len() - 1].to_string(),
            _ => input,
        }
    } else {
        input
    }
}
