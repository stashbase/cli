use anyhow::{bail, Result};

use crate::{
    cmd::{
        configs::OutputFormat,
        webhooks::{WebhookCommand, WebhookSubcommand},
    },
    handlers::webhooks::{
        create::{handle_create_webhook, CreateWebhookArgs},
        delete::{handle_delete_webhook, DeleteWebhookArgs},
        get::{handle_get_webhook, GetWebhookArgs},
        list::{handle_list_webhooks, ListWebhooksArgs},
        logs::{handle_list_webhook_logs, ListWebhookLogsArgs},
        open::handle_open_environment_webhook,
        rotate_secret::{handle_rotate_webhook_secret, RotateWebhookSecretArgs},
        test::{handle_test_webhook, TestWebhookArgs},
        update::{handle_update_webhook, UpdateWebhookArgs},
        update_status::{handle_update_webhook_status, UpdateWebhookStatusArgs},
    },
    utils::validation::{
        validate_project_environment, validate_webhook_description, validate_webhook_id,
        validate_webhook_url,
    },
};

fn validate_input(project: &str, environment: &str, subcommand: &WebhookSubcommand) -> Result<()> {
    // validate project and environment
    let input_valid = validate_project_environment(project, environment, true);

    if let Err(err) = input_valid {
        bail!(err);
    }

    // validate webhook id
    if let Some(webhook_id) = subcommand.get_webhook_id() {
        let valid_webhook_id = validate_webhook_id(webhook_id);
        if let Err(err) = valid_webhook_id {
            bail!(err);
        }
    }

    if let Some(webhook_id) = subcommand.get_webhook_url() {
        let valid_webhook_url = validate_webhook_url(webhook_id);
        if let Err(err) = valid_webhook_url {
            bail!(err);
        }
    }

    if let Some(description) = subcommand.get_description() {
        let valid_description = validate_webhook_description(description);

        if let Err(err) = valid_description {
            bail!(err);
        }
    }

    Ok(())
}

pub async fn handle_webhook_commands(
    cmd: WebhookCommand,
    api_key: String,
    raw_output: bool,
    default_output_format: Option<OutputFormat>,
) -> Result<()> {
    // required options
    let (project, environment) = cmd.try_get_project_environment()?;

    // other input
    validate_input(&project, &environment, &cmd.subcommand)?;

    match cmd.subcommand {
        WebhookSubcommand::List(args) => {
            let args = ListWebhooksArgs {
                api_key,
                project,
                environment,
                format: match raw_output {
                    true => OutputFormat::Json,
                    false => args
                        .format
                        .unwrap_or(default_output_format.unwrap_or_default()),
                },
            };

            handle_list_webhooks(args).await?;
        }
        WebhookSubcommand::Get(args) => {
            let args = GetWebhookArgs {
                api_key,
                project,
                environment,
                webhook_id: args.webhook_id,
                with_secret: args.with_secret,
                format: match raw_output {
                    true => OutputFormat::Json,
                    false => args
                        .format
                        .unwrap_or(default_output_format.unwrap_or_default()),
                },
            };

            handle_get_webhook(args).await?;
        }
        WebhookSubcommand::Delete(cmd_args) => {
            let fn_args = DeleteWebhookArgs {
                api_key,
                project,
                environment,
                webhook_id: cmd_args.webhook_id,
            };

            handle_delete_webhook(fn_args).await?;
        }
        WebhookSubcommand::Create(cmd_args) => {
            let fn_args = CreateWebhookArgs {
                api_key,
                project,
                environment,
                url: cmd_args.url,
                description: cmd_args.description,
                return_secret: cmd_args.return_secret,
                enable: cmd_args.enable,
            };

            handle_create_webhook(fn_args).await?;
        }
        WebhookSubcommand::Update(cmd_args) => {
            let fn_args = UpdateWebhookArgs {
                api_key,
                project,
                environment,
                url: cmd_args.url,
                description: cmd_args.description,
                webhook_id: cmd_args.webhook_id,
            };

            handle_update_webhook(fn_args).await?;
        }
        // update status
        WebhookSubcommand::Disable(cmd_args) => {
            let fn_args = UpdateWebhookStatusArgs {
                api_key,
                project,
                environment,
                webhook_id: cmd_args.webhook_id,
                enabled: false,
            };

            handle_update_webhook_status(fn_args).await?;
        }
        WebhookSubcommand::Enable(cmd_args) => {
            let fn_args = UpdateWebhookStatusArgs {
                api_key,
                project,
                environment,
                webhook_id: cmd_args.webhook_id,
                enabled: true,
            };

            handle_update_webhook_status(fn_args).await?;
        }
        WebhookSubcommand::Test(cmd_args) => {
            let fn_args = TestWebhookArgs {
                api_key,
                project,
                environment,
                webhook_id: cmd_args.webhook_id,
            };

            handle_test_webhook(fn_args).await?;
        }

        WebhookSubcommand::Logs(cmd_args) => {
            let fn_args = ListWebhookLogsArgs {
                api_key,
                project,
                environment,
                webhook_id: cmd_args.webhook_id,
                page: cmd_args.page,
                per_page: cmd_args.per_page,
                format: match raw_output {
                    true => OutputFormat::Json,
                    false => cmd_args
                        .format
                        .unwrap_or(default_output_format.unwrap_or_default()),
                },
            };

            handle_list_webhook_logs(fn_args).await?;
        }
        WebhookSubcommand::Open(cmd_args) => {
            handle_open_environment_webhook(api_key, project, environment, cmd_args.webhook_id)
                .await?;
        }
        WebhookSubcommand::RotateSecret(cmd_args) => {
            let fn_args = RotateWebhookSecretArgs {
                api_key,
                project,
                environment,
                webhook_id: cmd_args.webhook_id,
            };

            handle_rotate_webhook_secret(fn_args).await?;
        }
    }

    Ok(())
}
