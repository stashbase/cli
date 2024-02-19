use owo_colors::OwoColorize;
use std::fmt::Display;

use serde::{Deserialize, Serialize};
use tabled::Tabled;

#[derive(Debug, Serialize, Deserialize, Tabled)]
#[serde(rename_all = "camelCase")]
pub struct ListWebhook {
    #[tabled(order = 0)]
    id: String,

    #[tabled(order = 1)]
    url: String,

    #[tabled(order = 2)]
    enabled: bool,
}

impl Display for ListWebhook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.enabled == true {
            writeln!(f, "{} {}", "Enabled:", "true".green())?;
        } else {
            writeln!(f, "{} {}", "Enabled:", "false".red())?;
        }

        writeln!(f, "{} {}", "Id:", self.id)?;
        writeln!(f, "{} {}", "URL:", self.url)?;

        Ok(())
    }
}
