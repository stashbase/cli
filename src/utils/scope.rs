use crate::cmd::shared::Scope;

/// Detect CLI scope from API key
pub fn detect_scope_from_api_key(api_key: &str) -> Scope {
    match api_key.starts_with("sbe_") {
        true => Scope::Environment,
        false => Scope::Workspace,
    }
}
