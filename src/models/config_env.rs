use core::fmt;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvConfigItem {
    pub project: String,
    pub environment: String,
    pub description: Option<String>,

    pub secrets: Option<EnvConfigItemSecrets>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvConfigItemSecrets {
    pub print: Option<bool>,
    // Select secret keys
    pub only: Option<Vec<String>>,
    // Exclude secret keys
    pub exclude: Option<Vec<String>>,
}

impl fmt::Display for EnvConfigItem {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let str = match &self.description {
            Some(description) => {
                format!(
                    "{} -> {}\n   🗎 {}",
                    self.project, self.environment, description
                )
                //     format!(
                //         "{} -> {}\n   📄{}",
                //         self.project, self.environment, description
                //     )
            }
            None => {
                format!("{} -> {}", self.project, self.environment)
            }
        };

        write!(f, "{}", str)?;

        Ok(())
    }
}
