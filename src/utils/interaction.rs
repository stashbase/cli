#![allow(dead_code)]

use dialoguer::{theme::ColorfulTheme, Confirm, Input, Password, Select};

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

pub fn input_password(prompt: &str) -> Option<String> {
    match Password::with_theme(&ColorfulTheme::default())
        .with_prompt(prompt)
        .allow_empty_password(true)
        .interact()
    {
        Ok(value) if !value.trim().is_empty() => Some(value.trim().to_string()),
        _ => None, // empty or error
    }
}
