use dialoguer::{theme::ColorfulTheme, Confirm, Input, Select};

pub fn confirm_opt(prompt: &str) -> Option<bool> {
    Confirm::with_theme(&ColorfulTheme::default())
        .with_prompt(prompt)
        .interact_opt()
        .unwrap_or_else(|_| None)
}

pub fn confirm_text(confirm_text: &str) -> String {
    Input::with_theme(&ColorfulTheme::default())
        .with_prompt(format!("Type '{}' to confirm", confirm_text))
        .validate_with(|input: &String| -> Result<(), &str> {
            if input == confirm_text {
                Ok(())
            } else {
                Err("Input does not match.")
            }
        })
        .interact()
        .unwrap_or_else(|_| format!(""))
}

pub fn input(prompt: &str) -> String {
    Input::with_theme(&ColorfulTheme::default())
        .with_prompt(prompt)
        .interact_text()
        .unwrap_or_else(|_| "".to_string())
}

pub fn select(prompt: &str, selections: Vec<String>) -> Option<usize> {
    Select::with_theme(&ColorfulTheme::default())
        .with_prompt(prompt)
        .default(0)
        .items(&selections[..])
        .interact_opt()
        .unwrap_or_else(|_| None)
}
