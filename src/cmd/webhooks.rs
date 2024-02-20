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

    /// Update webhook
    #[clap(aliases = &["u", "upd"])]
    Update(UpdateWebhook),

    /// Enable webhook
    Enable(UpdateWebhookStatus),

    /// Disable webhook
    Disable(UpdateWebhookStatus),

    /// Send test event
    Test(TestWebhook),

    #[clap(aliases = &["d", "del"])]
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
pub struct UpdateWebhook {
    /// Id of webhook
    pub webhook_id: String,

    /// Webhook URL
    #[arg(value_enum, short = 'u', long = "url")]
    pub url: Option<String>,

    /// Webhook description
    #[arg(value_enum, short = 'd', long = "description")]
    pub description: Option<String>,
}

#[derive(Debug, Args)]
pub struct DeleteWebhook {
    /// Id of webhook
    pub webhook_id: String,
}

#[derive(Debug, Args)]
pub struct UpdateWebhookStatus {
    /// Id of webhook
    pub webhook_id: String,
}

#[derive(Debug, Args)]
pub struct TestWebhook {
    /// Id of webhook
    pub webhook_id: String,
}
