use clap::{Args, Subcommand};

use super::environments::EnvironmentFormat;

#[derive(Debug, Args)]
pub struct WebhookCommand {
    /// Project name
    #[arg(value_enum, short = 'p', long = "project", required = true)]
    pub project: String,

    /// Environment name
    #[arg(value_enum, short = 'e', long = "environment", required = true)]
    pub environment: String,

    #[clap(subcommand)]
    pub subcommand: WebhookSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum WebhookSubcommand {
    /// List all webhooks
    #[clap(alias = "l")]
    List(ListWebhooks),

    /// Get single webhook
    Get(GetWebhook),

    /// Create new webhook
    Create(CreateWebhook),

    Delete(DeleteWebhook),
}

// TODO: sort
#[derive(Debug, Args)]
pub struct ListWebhooks {
    // #[arg(value_enum, short = 'p', long = "page")]
    // pub page: Option<usize>,
    /// Format output
    #[arg(value_enum, short = 'f', long = "format")]
    pub format: Option<EnvironmentFormat>,
}

#[derive(Debug, Args)]
pub struct GetWebhook {
    /// Id of webhook
    pub webhook_id: String,
}

#[derive(Debug, Args)]
pub struct CreateWebhook {
    /// URL endpoint
    pub url: String,

    /// Description
    #[arg(value_enum, short = 'd', long = "description")]
    pub description: Option<String>,
}

#[derive(Debug, Args)]
pub struct DeleteWebhook {
    /// Id of webhook
    pub webhook_id: String,
}
