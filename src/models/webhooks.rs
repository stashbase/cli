use owo_colors::OwoColorize;
use std::fmt::Display;

use serde::{Deserialize, Serialize};
use tabled::Tabled;

use crate::utils::human_datetime::get_human_datetime;

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

// NOTE: with details
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Webhook {
    url: String,
    enabled: bool,

    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,

    created_at: String,
    // created_by: string
}

impl Display for Webhook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.enabled == true {
            writeln!(f, "{} {}", "Enabled:", "true".green())?;
        } else {
            writeln!(f, "{} {}", "Enabled:", "false".red())?;
        }

        let (formatted, relative) = get_human_datetime(&self.created_at);
        writeln!(f, "{} {} ({})", "Created at:", formatted, relative)?;

        writeln!(f, "{} {}", "URL:", self.url)?;

        if let Some(description) = &self.description {
            writeln!(f, "{} {}", "Description:", description)?;
        }

        Ok(())
    }
}

// impl Display for Environment {
//     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//         writeln!(f, "{} {}", "Env name:".green(), self.name)?;
//
//         let (formatted, relative) = get_human_datetime(&self.created_at);
//
//         writeln!(f, "{} {} ({})", "Created at:".green(), formatted, relative)?;
//         writeln!(f, "{} {}", "Type:".green(), self.env_type)?;
//         writeln!(f, "{} {}", "Locked:".green(), self.locked)?;
//
//         if let Some(description) = &self.description {
//             writeln!(f, "{} {}", "Description:".green(), description)?;
//         }
//
//         writeln!(f, "{} {}", "Secret count:".green(), self.secret_count)?;
//
//         Ok(())
//     }
// }
