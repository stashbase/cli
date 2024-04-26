use anyhow::{bail, Result};
use clap::{Args, Subcommand};

use super::{
    environments::EnvironmentFormat,
    shared::{self, RequiredArgs, SharedProjectEnvArgs},
};
use crate::models::validation::{CmdArgInputValidationError, InputValidationError};

#[derive(Debug, Args)]
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

impl RequiredArgs for WebhookCommand {
    fn try_get_project_environment(&self) -> anyhow::Result<(String, String)> {
        let root_project: Option<_> = self.project.as_deref();
        let root_environment: Option<_> = self.environment.as_deref();

        let (project, environment) = self.subcommand.get_project_environment();

        shared::try_get_project_environment(root_project, root_environment, project, environment)
    }
}

impl WebhookCommand {
    pub fn try_get_project_environment(&self) -> Result<(String, String)> {
        let root_project: Option<_> = self.project.as_deref();
        let root_environment: Option<_> = self.environment.as_deref();

        let (project, environment) = self.subcommand.get_project_environment();

        if root_project.is_some() && project.is_some() {
            bail!(InputValidationError::CmdArgs(
                CmdArgInputValidationError::DuplicateProject
            ))
        }

        if root_environment.is_some() && environment.is_some() {
            bail!(InputValidationError::CmdArgs(
                CmdArgInputValidationError::DuplicateEnvironment
            ))
        }

        if project.is_none()
            && root_project.is_none()
            && environment.is_none()
            && root_environment.is_none()
        {
            bail!(InputValidationError::CmdArgs(
                CmdArgInputValidationError::MissingProjectEnvironment
            ))
        }

        if project.is_none() && root_project.is_none() {
            bail!(InputValidationError::CmdArgs(
                CmdArgInputValidationError::MissingProject
            ))
        }

        if environment.is_none() && root_environment.is_none() {
            bail!(InputValidationError::CmdArgs(
                CmdArgInputValidationError::MissingEnvironment
            ))
        }

        let project = match root_project {
            Some(p) => p.to_string(),
            None => project.unwrap(),
        };

        let environment = match root_environment {
            Some(e) => e.to_string(),
            None => environment.unwrap(),
        };

        return Ok((project, environment));
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
    Enable(SingleWebhook),

    /// Disable webhook
    Disable(SingleWebhook),

    /// Send test event
    Test(SingleWebhook),

    /// Rotate signing secret
    RotateSecret(SingleWebhook),

    /// Delete webhook
    #[clap(aliases = &["d", "del"])]
    Delete(SingleWebhook),

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

    fn get_project_environment(&self) -> (Option<String>, Option<String>) {
        match self {
            WebhookSubcommand::List(l) => (
                l.shared_args.project.to_owned(),
                l.shared_args.environment.to_owned(),
            ),
            WebhookSubcommand::Get(g) => (
                g.shared_args.project.to_owned(),
                g.shared_args.environment.to_owned(),
            ),
            WebhookSubcommand::Create(c) => (
                c.shared_args.project.to_owned(),
                c.shared_args.environment.to_owned(),
            ),
            WebhookSubcommand::Update(u) => (
                u.shared_args.project.to_owned(),
                u.shared_args.environment.to_owned(),
            ),
            WebhookSubcommand::Enable(e) => (
                e.shared_args.project.to_owned(),
                e.shared_args.environment.to_owned(),
            ),
            WebhookSubcommand::Disable(d) => (
                d.shared_args.project.to_owned(),
                d.shared_args.environment.to_owned(),
            ),
            WebhookSubcommand::Test(t) => (
                t.shared_args.project.to_owned(),
                t.shared_args.environment.to_owned(),
            ),
            WebhookSubcommand::RotateSecret(r) => (
                r.shared_args.project.to_owned(),
                r.shared_args.environment.to_owned(),
            ),
            WebhookSubcommand::Delete(d) => (
                d.shared_args.project.to_owned(),
                d.shared_args.environment.to_owned(),
            ),
            WebhookSubcommand::Logs(l) => (
                l.shared_args.project.to_owned(),
                l.shared_args.environment.to_owned(),
            ),
            WebhookSubcommand::Open(o) => (
                o.shared_args.project.to_owned(),
                o.shared_args.environment.to_owned(),
            ),
        }
    }
}

// TODO: sort
#[derive(Debug, Args)]
pub struct ListWebhooks {
    #[clap(flatten)]
    pub shared_args: SharedProjectEnvArgs,

    // #[arg(value_enum, short = 'p', long = "page")]
    // pub page: Option<usize>,
    /// Format output
    #[arg(value_enum, short = 'f', long = "format")]
    pub format: Option<EnvironmentFormat>,
}

#[derive(Debug, Args)]
pub struct GetWebhook {
    #[clap(flatten)]
    pub shared_args: SharedProjectEnvArgs,

    /// Id of webhook
    pub webhook_id: String,

    /// With signing secret
    #[arg(value_enum, long = "secret")]
    pub with_secret: bool,
}

#[derive(Debug, Args)]
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
pub struct WebhookLogs {
    #[clap(flatten)]
    pub shared_args: SharedProjectEnvArgs,

    /// Id of webhook
    pub webhook_id: String,

    /// Format output
    #[arg(value_enum, short = 'f', long = "format")]
    pub format: Option<EnvironmentFormat>,

    // TODO: describe
    #[arg(value_enum, short = 'p', long = "page")]
    pub page: Option<usize>,

    // TODO: describe
    #[arg(value_enum, long = "per-page")]
    pub per_page: Option<u8>,
}

#[derive(Debug, Args)]
pub struct OpenWebhooks {
    #[clap(flatten)]
    pub shared_args: SharedProjectEnvArgs,

    /// Id of webhook
    pub webhook_id: Option<String>,
}

#[derive(Debug, Args)]
pub struct SingleWebhook {
    #[clap(flatten)]
    pub shared_args: SharedProjectEnvArgs,

    /// Id of webhook
    pub webhook_id: String,
}
