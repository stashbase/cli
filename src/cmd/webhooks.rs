use anyhow::Result;
use clap::{Args, Subcommand};

use super::{
    config::OutputFormat,
    shared::{try_get_project_environment, SharedProjectEnvArgs},
};

#[derive(Debug, Args)]
#[command(override_usage = "webhooks <COMMAND> -p <PROJECT> -e <ENVIRONMENT> [OPTIONS]")]
pub struct WebhookCommand {
    /// Project name
    #[arg(value_enum, short = 'p', long = "project", required = false)]
    pub project: Option<String>,

    /// Environment name
    #[arg(value_enum, short = 'e', long = "environment", required = false)]
    pub environment: Option<String>,

    #[clap(subcommand)]
    pub subcommand: WebhookSubcommand,
}

impl WebhookCommand {
    pub fn try_get_project_environment(&self) -> Result<(String, String)> {
        let root_project: Option<_> = self.project.as_deref();
        let root_environment: Option<_> = self.environment.as_deref();

        let (project, environment) = self.subcommand.get_project_environment();

        try_get_project_environment(root_project, root_environment, project, environment)
    }
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
    Enable(SetEnableStatus),

    /// Disable webhook
    Disable(SetEnableStatus),

    /// Send test event
    Test(TestWebhook),

    /// Get signing secret
    GetSecret(GetSigningSecret),

    /// Rotate signing secret
    RotateSecret(RoateteWebhookSecret),

    /// Delete webhook
    #[clap(aliases = &["d", "del"])]
    Delete(DeleteWebhook),

    /// List webhook logs
    Logs(WebhookLogs),

    /// Open environment webhook in browser
    Open(OpenWebhooks),
}

impl WebhookSubcommand {
    pub fn get_webhook_id(&self) -> Option<&str> {
        match self {
            WebhookSubcommand::Get(cmd) => Some(&cmd.webhook_id),
            WebhookSubcommand::Delete(cmd) => Some(&cmd.webhook_id),
            WebhookSubcommand::Logs(cmd) => Some(&cmd.webhook_id),
            WebhookSubcommand::Test(cmd) => Some(&cmd.webhook_id),
            WebhookSubcommand::Update(cmd) => Some(&cmd.webhook_id),
            WebhookSubcommand::Enable(cmd) => Some(&cmd.webhook_id),
            WebhookSubcommand::Disable(cmd) => Some(&cmd.webhook_id),
            WebhookSubcommand::Open(cmd) => match &cmd.webhook_id {
                Some(webhook_id) => Some(webhook_id),
                None => None,
            },
            _ => None,
        }
    }

    pub fn get_webhook_url(&self) -> Option<&str> {
        match self {
            WebhookSubcommand::Create(cmd) => Some(&cmd.url),
            WebhookSubcommand::Update(cmd) => cmd.url.as_deref(),
            _ => None,
        }
    }

    pub fn get_description(&self) -> Option<&str> {
        match self {
            WebhookSubcommand::Create(cmd) => cmd.description.as_deref(),
            WebhookSubcommand::Update(cmd) => cmd.description.as_deref(),
            _ => None,
        }
    }

    fn get_project_environment(&self) -> (Option<&str>, Option<&str>) {
        match self {
            WebhookSubcommand::List(l) => (
                l.shared_args.project.as_deref(),
                l.shared_args.environment.as_deref(),
            ),
            WebhookSubcommand::Get(g) => (
                g.shared_args.project.as_deref(),
                g.shared_args.environment.as_deref(),
            ),
            WebhookSubcommand::Create(c) => (
                c.shared_args.project.as_deref(),
                c.shared_args.environment.as_deref(),
            ),
            WebhookSubcommand::Update(u) => (
                u.shared_args.project.as_deref(),
                u.shared_args.environment.as_deref(),
            ),
            WebhookSubcommand::Enable(e) => (
                e.shared_args.project.as_deref(),
                e.shared_args.environment.as_deref(),
            ),
            WebhookSubcommand::Disable(d) => (
                d.shared_args.project.as_deref(),
                d.shared_args.environment.as_deref(),
            ),
            WebhookSubcommand::Test(t) => (
                t.shared_args.project.as_deref(),
                t.shared_args.environment.as_deref(),
            ),
            WebhookSubcommand::RotateSecret(r) => (
                r.shared_args.project.as_deref(),
                r.shared_args.environment.as_deref(),
            ),
            WebhookSubcommand::Delete(d) => (
                d.shared_args.project.as_deref(),
                d.shared_args.environment.as_deref(),
            ),
            WebhookSubcommand::Logs(l) => (
                l.shared_args.project.as_deref(),
                l.shared_args.environment.as_deref(),
            ),
            WebhookSubcommand::Open(o) => (
                o.shared_args.project.as_deref(),
                o.shared_args.environment.as_deref(),
            ),
            WebhookSubcommand::GetSecret(s) => (
                s.shared_args.project.as_deref(),
                s.shared_args.environment.as_deref(),
            ),
        }
    }
}

// TODO: sort
#[derive(Debug, Args)]
#[command(override_usage = "webhooks list -p <PROJECT> -e <ENVIRONMENT> [OPTIONS]")]
pub struct ListWebhooks {
    #[clap(flatten)]
    pub shared_args: SharedProjectEnvArgs,

    // #[arg(value_enum, short = 'p', long = "page")]
    // pub page: Option<usize>,
    /// Format output
    #[arg(value_enum, short = 'f', long = "format")]
    pub format: Option<OutputFormat>,
}

#[derive(Debug, Args)]
#[command(override_usage = "webhooks get <WEBHOOK_ID> -p <PROJECT> -e <ENVIRONMENT> [OPTIONS]")]
pub struct GetWebhook {
    #[clap(flatten)]
    pub shared_args: SharedProjectEnvArgs,

    /// Id of webhook
    pub webhook_id: String,

    /// With signing secret
    #[arg(value_enum, long = "with-secret")]
    pub with_secret: bool,

    /// Format output
    #[arg(value_enum, short = 'f', long = "format")]
    pub format: Option<OutputFormat>,
}

#[derive(Debug, Args)]
#[command(
    override_usage = "webhooks get-secret <WEBHOOK_ID> -p <PROJECT> -e <ENVIRONMENT> [OPTIONS]"
)]
pub struct GetSigningSecret {
    #[clap(flatten)]
    pub shared_args: SharedProjectEnvArgs,

    /// Id of webhook
    pub webhook_id: String,
}

#[derive(Debug, Args)]
#[command(override_usage = "webhooks delete <WEBHOOK_ID> -p <PROJECT> -e <ENVIRONMENT> [OPTIONS]")]
pub struct DeleteWebhook {
    #[clap(flatten)]
    pub shared_args: SharedProjectEnvArgs,

    /// Id of webhook
    pub webhook_id: String,
}

#[derive(Debug, Args)]
#[command(override_usage = "webhooks test <WEBHOOK_ID> -p <PROJECT> -e <ENVIRONMENT> [OPTIONS]")]
pub struct TestWebhook {
    #[clap(flatten)]
    pub shared_args: SharedProjectEnvArgs,

    /// Id of webhook
    pub webhook_id: String,
}

#[derive(Debug, Args)]
#[command(override_usage = "webhooks create <URL> -p <PROJECT> -e <ENVIRONMENT> [OPTIONS]")]
pub struct CreateWebhook {
    #[clap(flatten)]
    pub shared_args: SharedProjectEnvArgs,

    /// URL endpoint
    pub url: String,

    /// Description
    #[arg(value_enum, short = 'd', long = "description")]
    pub description: Option<String>,

    /// Return signing secret
    #[arg(value_enum, long = "enable")]
    pub enable: bool,

    /// Return signing secret
    #[arg(value_enum, long = "return-secret")]
    pub return_secret: bool,
}

#[derive(Debug, Args)]
#[command(override_usage = "webhooks update <WEBHOOK_ID> -p <PROJECT> -e <ENVIRONMENT> [OPTIONS]")]
pub struct UpdateWebhook {
    #[clap(flatten)]
    pub shared_args: SharedProjectEnvArgs,

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
#[command(override_usage = "webhooks logs <WEBHOOK_ID> -p <PROJECT> -e <ENVIRONMENT> [OPTIONS]")]
pub struct WebhookLogs {
    #[clap(flatten)]
    pub shared_args: SharedProjectEnvArgs,

    /// Id of webhook
    pub webhook_id: String,

    /// Format output
    #[arg(value_enum, short = 'f', long = "format")]
    pub format: Option<OutputFormat>,

    /// Page number
    #[arg(value_enum, long = "page")]
    pub page: Option<usize>,

    /// Items per page
    #[arg(value_enum, long = "limit")]
    pub limit: Option<u8>,
}

#[derive(Debug, Args)]
#[command(override_usage = "webhooks open [WEBHOOK_ID] -p <PROJECT> -e <ENVIRONMENT>")]
pub struct OpenWebhooks {
    #[clap(flatten)]
    pub shared_args: SharedProjectEnvArgs,

    /// Id of webhook
    pub webhook_id: Option<String>,
}

#[derive(Debug, Args)]
#[command(override_usage = "webhooks enable/disable <WEBHOOK_ID> -p <PROJECT> -e <ENVIRONMENT>")]
pub struct SetEnableStatus {
    #[clap(flatten)]
    pub shared_args: SharedProjectEnvArgs,

    /// Id of webhook
    pub webhook_id: String,
}

#[derive(Debug, Args)]
#[command(override_usage = "webhooks rotate-secret <WEBHOOK_ID> -p <PROJECT> -e <ENVIRONMENT>")]
pub struct RoateteWebhookSecret {
    #[clap(flatten)]
    pub shared_args: SharedProjectEnvArgs,

    /// Id of webhook
    pub webhook_id: String,
}
