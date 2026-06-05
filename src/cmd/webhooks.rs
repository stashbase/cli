use clap::{Args, Subcommand};

use crate::models::{scope::Scope, validation::InputValidationError};

use super::{
    config::OutputFormat,
    shared::{try_get_project_environment, try_get_scope, SharedProjectEnvArgs, SharedScopeArgs},
};

#[derive(Debug, Args)]
#[command(
    override_usage = "webhooks <COMMAND> (-p <PROJECT> -e <ENVIRONMENT> | --scope=environment) [OPTIONS]"
)]
pub struct WebhookCommand {
    /// Project name
    #[arg(short = 'p', long = "project", required = false)]
    pub project: Option<String>,

    /// Environment name
    #[arg(short = 'e', long = "environment", required = false)]
    pub environment: Option<String>,

    /// API scope [default: workspace]
    #[arg(long = "scope", value_enum)]
    pub scope: Option<Scope>,

    #[clap(subcommand)]
    pub subcommand: WebhookSubcommand,
}

impl WebhookCommand {
    pub fn try_get_project_environment(&self) -> Result<(String, String), InputValidationError> {
        let root_project: Option<_> = self.project.as_deref();
        let root_environment: Option<_> = self.environment.as_deref();

        let (project, environment) = self.subcommand.get_project_environment();

        try_get_project_environment(root_project, root_environment, project, environment)
    }

    pub fn get_scope(&self) -> Result<Option<Scope>, InputValidationError> {
        let root_scope = self.scope.as_ref();
        let subcommand_scope = self.subcommand.get_scope();

        try_get_scope(root_scope, subcommand_scope)
    }
}

#[derive(Debug, Subcommand)]
pub enum WebhookSubcommand {
    /// List all webhooks
    List(ListWebhooks),

    /// Get single webhook
    Get(GetWebhook),

    /// Create new webhook
    #[clap(name = "create", alias = "new")]
    Create(CreateWebhook),

    /// Update webhook
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
    #[clap(aliases = &["del"])]
    Delete(DeleteWebhook),

    /// Manage webhook logs
    Logs(WebhookLogsCommand),

    /// Open environment webhook in browser
    Open(OpenWebhooks),
}

impl WebhookSubcommand {
    pub fn get_webhook_id(&self) -> Option<&str> {
        match self {
            WebhookSubcommand::Get(cmd) => Some(&cmd.webhook_id),
            WebhookSubcommand::Delete(cmd) => Some(&cmd.webhook_id),
            WebhookSubcommand::Logs(cmd) => cmd.get_webhook_id(),
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
            WebhookSubcommand::Logs(l) => l.get_project_environment(),
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

    pub fn get_scope(&self) -> Option<&Scope> {
        match self {
            WebhookSubcommand::List(l) => l.scope_args.scope.as_ref(),
            WebhookSubcommand::Get(g) => g.scope_args.scope.as_ref(),
            WebhookSubcommand::Create(c) => c.scope_args.scope.as_ref(),
            WebhookSubcommand::Update(u) => u.scope_args.scope.as_ref(),
            WebhookSubcommand::Enable(e) => e.scope_args.scope.as_ref(),
            WebhookSubcommand::Disable(d) => d.scope_args.scope.as_ref(),
            WebhookSubcommand::Test(t) => t.scope_args.scope.as_ref(),
            WebhookSubcommand::RotateSecret(r) => r.scope_args.scope.as_ref(),
            WebhookSubcommand::Delete(d) => d.scope_args.scope.as_ref(),
            WebhookSubcommand::Logs(l) => l.get_scope(),
            WebhookSubcommand::Open(o) => o.scope_args.scope.as_ref(),
            WebhookSubcommand::GetSecret(s) => s.scope_args.scope.as_ref(),
        }
    }
}

// TODO: sort
#[derive(Debug, Args)]
#[command(override_usage = "webhooks list [OPTIONS]")]
pub struct ListWebhooks {
    #[clap(flatten)]
    pub shared_args: SharedProjectEnvArgs,

    #[clap(flatten)]
    pub scope_args: SharedScopeArgs,

    // #[arg(value_enum, short = 'p', long = "page")]
    // pub page: Option<usize>,
    /// Format output
    #[arg(short = 'f', long = "format")]
    pub format: Option<OutputFormat>,
}

#[derive(Debug, Args)]
#[command(override_usage = "webhooks get <WEBHOOK_ID> [OPTIONS]")]
pub struct GetWebhook {
    #[clap(flatten)]
    pub shared_args: SharedProjectEnvArgs,

    #[clap(flatten)]
    pub scope_args: SharedScopeArgs,

    /// Id of webhook
    pub webhook_id: String,

    /// Include signing secret in output
    #[arg(long = "include-secret")]
    pub include_secret: bool,

    /// Format output
    #[arg(short = 'f', long = "format")]
    pub format: Option<OutputFormat>,
}

#[derive(Debug, Args)]
#[command(override_usage = "webhooks get-secret <WEBHOOK_ID> [OPTIONS]")]
pub struct GetSigningSecret {
    #[clap(flatten)]
    pub shared_args: SharedProjectEnvArgs,

    #[clap(flatten)]
    pub scope_args: SharedScopeArgs,

    /// Id of webhook
    pub webhook_id: String,
}

#[derive(Debug, Args)]
#[command(override_usage = "webhooks delete <WEBHOOK_ID> [OPTIONS]")]
pub struct DeleteWebhook {
    #[clap(flatten)]
    pub shared_args: SharedProjectEnvArgs,

    #[clap(flatten)]
    pub scope_args: SharedScopeArgs,

    /// Id of webhook
    pub webhook_id: String,

    /// Proceed without confirmation
    #[arg(long = "force")]
    pub force: bool,
}

#[derive(Debug, Args)]
#[command(override_usage = "webhooks test <WEBHOOK_ID> [OPTIONS]")]
pub struct TestWebhook {
    #[clap(flatten)]
    pub shared_args: SharedProjectEnvArgs,

    #[clap(flatten)]
    pub scope_args: SharedScopeArgs,

    /// Id of webhook
    pub webhook_id: String,
}

#[derive(Debug, Args)]
#[command(override_usage = "webhooks create <URL> [OPTIONS]")]
pub struct CreateWebhook {
    #[clap(flatten)]
    pub shared_args: SharedProjectEnvArgs,

    #[clap(flatten)]
    pub scope_args: SharedScopeArgs,

    /// URL endpoint
    pub url: String,

    /// Description
    #[arg(short = 'd', long = "description")]
    pub description: Option<String>,

    /// Enable webhook on create
    #[arg(long = "enable")]
    pub enable: bool,
}

#[derive(Debug, Args)]
#[command(override_usage = "webhooks update <WEBHOOK_ID> [OPTIONS]")]
pub struct UpdateWebhook {
    #[clap(flatten)]
    pub shared_args: SharedProjectEnvArgs,

    #[clap(flatten)]
    pub scope_args: SharedScopeArgs,

    /// Id of webhook
    pub webhook_id: String,

    /// Webhook URL
    #[arg(short = 'u', long = "url")]
    pub url: Option<String>,

    /// Webhook description
    #[arg(short = 'd', long = "description")]
    pub description: Option<String>,
}

#[derive(Debug, Args)]
#[command(override_usage = "webhooks logs <COMMAND> [OPTIONS]")]
pub struct WebhookLogsCommand {
    #[clap(subcommand)]
    pub subcommand: WebhookLogsSubcommand,
}

impl WebhookLogsCommand {
    fn get_webhook_id(&self) -> Option<&str> {
        match &self.subcommand {
            WebhookLogsSubcommand::List(cmd) => Some(&cmd.webhook_id),
        }
    }

    fn get_project_environment(&self) -> (Option<&str>, Option<&str>) {
        match &self.subcommand {
            WebhookLogsSubcommand::List(cmd) => (
                cmd.shared_args.project.as_deref(),
                cmd.shared_args.environment.as_deref(),
            ),
        }
    }

    fn get_scope(&self) -> Option<&Scope> {
        match &self.subcommand {
            WebhookLogsSubcommand::List(cmd) => cmd.scope_args.scope.as_ref(),
        }
    }
}

#[derive(Debug, Subcommand)]
pub enum WebhookLogsSubcommand {
    /// List webhook logs
    List(WebhookLogs),
}

#[derive(Debug, Args)]
#[command(override_usage = "webhooks logs list <WEBHOOK_ID> [OPTIONS]")]
pub struct WebhookLogs {
    #[clap(flatten)]
    pub shared_args: SharedProjectEnvArgs,

    #[clap(flatten)]
    pub scope_args: SharedScopeArgs,

    /// Id of webhook
    pub webhook_id: String,

    /// Format output
    #[arg(short = 'f', long = "format")]
    pub format: Option<OutputFormat>,

    /// Page number
    #[arg(long = "page")]
    pub page: Option<usize>,

    /// Items per page
    #[arg(long = "page-size")]
    pub page_size: Option<usize>,
}

#[derive(Debug, Args)]
#[command(override_usage = "webhooks open <WEBHOOK_ID> [OPTIONS]")]
pub struct OpenWebhooks {
    #[clap(flatten)]
    pub shared_args: SharedProjectEnvArgs,

    #[clap(flatten)]
    pub scope_args: SharedScopeArgs,

    /// Id of webhook
    pub webhook_id: Option<String>,
}

#[derive(Debug, Args)]
#[command(override_usage = "webhooks enable/disable <WEBHOOK_ID> [OPTIONS]")]
pub struct SetEnableStatus {
    #[clap(flatten)]
    pub shared_args: SharedProjectEnvArgs,

    #[clap(flatten)]
    pub scope_args: SharedScopeArgs,

    /// Id of webhook
    pub webhook_id: String,

    /// Proceed without confirmation
    #[arg(long = "force")]
    pub force: bool,
}

#[derive(Debug, Args)]
#[command(override_usage = "webhooks rotate-secret <WEBHOOK_ID> [OPTIONS]")]
pub struct RoateteWebhookSecret {
    #[clap(flatten)]
    pub shared_args: SharedProjectEnvArgs,

    #[clap(flatten)]
    pub scope_args: SharedScopeArgs,

    /// Id of webhook
    pub webhook_id: String,

    /// Proceed without confirmation
    #[arg(long = "force")]
    pub force: bool,
}
