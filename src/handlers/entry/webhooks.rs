use anyhow::bail;

use crate::{
    cmd::{
        config::OutputFormat,
        webhooks::{WebhookCommand, WebhookSubcommand},
    },
    handlers::webhooks::{
        create::{handle_create_webhook, CreateWebhookArgs},
        delete::{handle_delete_webhook, DeleteWebhookArgs},
        get::{handle_get_webhook, GetWebhookArgs},
        get_secret::{handle_get_webhook_secret, GetWebhookSecretArgs},
        list::{handle_list_webhooks, ListWebhooksArgs},
        logs::{handle_list_webhook_logs, ListWebhookLogsArgs},
        open::handle_open_environment_webhook,
        rotate_secret::{handle_rotate_webhook_secret, RotateWebhookSecretArgs},
        test::{handle_test_webhook, TestWebhookArgs},
        update::{handle_update_webhook, UpdateWebhookArgs},
        update_status::{handle_update_webhook_status, UpdateWebhookStatusArgs},
    },
    models::validation::InputValidationError,
    utils::{
        output::get_output_format,
        validation::{
            validate_project_environment_identifier, validate_webhook_description,
            validate_webhook_id, validate_webhook_url,
        },
    },
};

fn validate_input(
    project: &str,
    environment: &str,
    subcommand: &WebhookSubcommand,
) -> Result<(), InputValidationError> {
    // validate project and environment
    let input_valid = validate_project_environment_identifier(project, environment, false);

    if let Err(err) = input_valid {
        return Err(err);
    }

    // validate webhook id
    if let Some(webhook_id) = subcommand.get_webhook_id() {
        let valid_webhook_id = validate_webhook_id(webhook_id);
        if let Err(err) = valid_webhook_id {
            return Err(err);
        }
    }

    if let Some(webhook_id) = subcommand.get_webhook_url() {
        let valid_webhook_url = validate_webhook_url(webhook_id);
        if let Err(err) = valid_webhook_url {
            return Err(err);
        }
    }

    if let Some(description) = subcommand.get_description() {
        let valid_description = validate_webhook_description(description);

        if let Err(err) = valid_description {
            return Err(err);
        }
    }

    Ok(())
}

pub async fn handle_webhook_commands(
    cmd: WebhookCommand,
    api_key: String,
    silent: bool,
    raw_output: bool,
    default_output_format: Option<OutputFormat>,
) -> anyhow::Result<()> {
    // required options
    let project_env_res = cmd.try_get_project_environment();

    if let Err(err) = project_env_res {
        let formatted_err = err.format_error_output(raw_output)?;

        if !silent {
            eprintln!();
        }
        bail!(formatted_err);
    }

    let (project, environment) = project_env_res.unwrap();

    // other input
    let validation_res = validate_input(&project, &environment, &cmd.subcommand);

    if let Err(err) = validation_res {
        let formatted_err = err.format_error_output(raw_output)?;

        if !silent {
            eprintln!();
        }
        bail!(formatted_err);
    }

    match cmd.subcommand {
        WebhookSubcommand::List(args) => {
            let format = get_output_format(raw_output, default_output_format, args.format);

            let args = ListWebhooksArgs {
                api_key,
                project,
                environment,
                format,
                silent,
            };

            handle_list_webhooks(args).await?;
        }
        WebhookSubcommand::Get(args) => {
            let format = get_output_format(raw_output, default_output_format, args.format);

            let args = GetWebhookArgs {
                api_key,
                project,
                environment,
                webhook_id: args.webhook_id,
                with_secret: args.with_secret,
                format,
                silent,
            };

            handle_get_webhook(args).await?;
        }
        WebhookSubcommand::Delete(cmd_args) => {
            let fn_args = DeleteWebhookArgs {
                api_key,
                project,
                environment,
                silent,
                force: cmd_args.force,
                json_format: raw_output,
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
                json_format: raw_output,
                silent,
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
                json_format: raw_output,
                silent,
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
                json_format: raw_output,
                silent,
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
                json_format: raw_output,
                silent,
            };

            handle_update_webhook_status(fn_args).await?;
        }
        WebhookSubcommand::Test(cmd_args) => {
            let fn_args = TestWebhookArgs {
                api_key,
                project,
                environment,
                webhook_id: cmd_args.webhook_id,
                json_format: raw_output,
                silent,
            };

            handle_test_webhook(fn_args).await?;
        }

        WebhookSubcommand::Logs(cmd_args) => {
            let format = get_output_format(raw_output, default_output_format, cmd_args.format);

            let fn_args = ListWebhookLogsArgs {
                api_key,
                project,
                environment,
                webhook_id: cmd_args.webhook_id,
                page: cmd_args.page,
                limit: cmd_args.limit,
                format,
                silent,
            };

            handle_list_webhook_logs(fn_args).await?;
        }
        WebhookSubcommand::Open(cmd_args) => {
            handle_open_environment_webhook(
                api_key,
                project,
                environment,
                cmd_args.webhook_id,
                raw_output,
                silent,
            )
            .await?;
        }
        WebhookSubcommand::RotateSecret(cmd_args) => {
            let fn_args = RotateWebhookSecretArgs {
                api_key,
                project,
                environment,
                silent,
                webhook_id: cmd_args.webhook_id,
                json_format: raw_output,
                force: cmd_args.force,
            };

            handle_rotate_webhook_secret(fn_args).await?;
        }
        WebhookSubcommand::GetSecret(cmd_args) => {
            let fn_args = GetWebhookSecretArgs {
                api_key,
                project,
                environment,
                webhook_id: cmd_args.webhook_id,
                json_format: raw_output,
                silent,
            };

            handle_get_webhook_secret(fn_args).await?;
        }
    }

    Ok(())
}
