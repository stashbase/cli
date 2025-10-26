use std::env;

/// Returns the value of an environment variable if set and non-empty.
pub fn get_env_var(key: &str) -> Option<String> {
    match env::var(key) {
        Ok(val) if !val.trim().is_empty() => Some(val.trim().to_string()),
        _ => None,
    }
}

/// Convenience helper to get the Stashbase API key from environment.
pub fn get_stashbase_api_key() -> Option<String> {
    get_env_var("STASHBASE_API_KEY")
}
