use anyhow::bail;

use crate::{
    cmd::{
        environments::EnvironmentFormat,
        webhooks::{WebhookCommand, WebhookSubcommand},
    },
    handlers::webhooks::{
        create::{handle_create_webhook, CreateWebhookArgs},
        delete::{handle_delete_webhook, DeleteWebhookArgs},
        get::{handle_get_webhook, GetWebhookArgs},
        list::{handle_list_webhooks, ListWebhooksArgs},
        logs::{handle_list_webhook_logs, ListWebhookLogsArgs},
        open::handle_open_environment_webhook,
        test::{handle_test_webhook, TestWebhookArgs},
        update::{handle_update_webhook, UpdateWebhookArgs},
        update_status::{handle_update_webhook_status, UpdateWebhookStatusArgs},
    },
    utils::validation::{validate_project_environment, validate_webhook_id},
};

fn validate_input(cmd: &WebhookCommand) -> anyhow::Result<()> {
    // validate project and environment
    let input_valid = validate_project_environment(&cmd.project, &cmd.environment, true);

    if let Err(err) = input_valid {
        bail!(err);
    }

    // validate webhook id
    if let Some(ref webhook_id) = cmd.subcommand.get_webhook_id() {
        let valid_webhook_id = validate_webhook_id(webhook_id);
        if let Err(err) = valid_webhook_id {
            bail!(err);
        }
    }

    Ok(())
}

pub async fn handle_webhook_commands(cmd: WebhookCommand, api_key: String, raw_output: bool) {
    let input_valid = validate_input(&cmd);

    if let Err(err) = input_valid {
        eprintln!("{}", err);
        return;
    }

    match cmd.subcommand {
        WebhookSubcommand::List(args) => {
            let args = ListWebhooksArgs {
                api_key,
                project: cmd.project,
                environment: cmd.environment,
                format: match raw_output {
                    true => EnvironmentFormat::Json,
                    false => args.format.unwrap_or_default(),
                },
            };

            handle_list_webhooks(args).await.unwrap_or_else(|err| {
                eprintln!("{}", err);
            });
        }
        WebhookSubcommand::Get(args) => {
            let args = GetWebhookArgs {
                api_key,
                project: cmd.project,
                environment: cmd.environment,
                webhook_id: args.webhook_id,
                with_secret: args.with_secret,
                format_json: raw_output,
            };

            handle_get_webhook(args).await.unwrap_or_else(|err| {
                eprintln!("{}", err);
            })
        }
        WebhookSubcommand::Delete(cmd_args) => {
            let fn_args = DeleteWebhookArgs {
                api_key,
                project: cmd.project,
                environment: cmd.environment,
                webhook_id: cmd_args.webhook_id,
            };

            handle_delete_webhook(fn_args).await.unwrap_or_else(|err| {
                eprintln!("{}", err);
            })
        }
        WebhookSubcommand::Create(cmd_args) => {
            let fn_args = CreateWebhookArgs {
                api_key,
                project: cmd.project,
                environment: cmd.environment,
                url: cmd_args.url,
                description: cmd_args.description,
            };

            handle_create_webhook(fn_args).await.unwrap_or_else(|err| {
                eprintln!("{}", err);
            })
        }
        WebhookSubcommand::Update(cmd_args) => {
            let fn_args = UpdateWebhookArgs {
                api_key,
                project: cmd.project,
                environment: cmd.environment,
                url: cmd_args.url,
                description: cmd_args.description,
                webhook_id: cmd_args.webhook_id,
            };

            handle_update_webhook(fn_args).await.unwrap_or_else(|err| {
                eprintln!("{}", err);
            })
        }
        // update status
        WebhookSubcommand::Disable(cmd_args) => {
            let fn_args = UpdateWebhookStatusArgs {
                api_key,
                project: cmd.project,
                environment: cmd.environment,
                webhook_id: cmd_args.webhook_id,
                enabled: false,
            };

            handle_update_webhook_status(fn_args)
                .await
                .unwrap_or_else(|err| {
                    eprintln!("{}", err);
                })
        }
        WebhookSubcommand::Enable(cmd_args) => {
            let fn_args = UpdateWebhookStatusArgs {
                api_key,
                project: cmd.project,
                environment: cmd.environment,
                webhook_id: cmd_args.webhook_id,
                enabled: true,
            };

            handle_update_webhook_status(fn_args)
                .await
                .unwrap_or_else(|err| {
                    eprintln!("{}", err);
                })
        }
        WebhookSubcommand::Test(cmd_args) => {
            let fn_args = TestWebhookArgs {
                api_key,
                project: cmd.project,
                environment: cmd.environment,
                webhook_id: cmd_args.webhook_id,
            };

            handle_test_webhook(fn_args).await.unwrap_or_else(|err| {
                eprintln!("{}", err);
            })
        }

        WebhookSubcommand::Logs(cmd_args) => {
            let fn_args = ListWebhookLogsArgs {
                api_key,
                project: cmd.project,
                environment: cmd.environment,
                webhook_id: cmd_args.webhook_id,
                page: cmd_args.page,
                per_page: cmd_args.per_page,
                format: match raw_output {
                    true => EnvironmentFormat::Json,
                    false => cmd_args.format.unwrap_or_default(),
                },
            };

            handle_list_webhook_logs(fn_args)
                .await
                .unwrap_or_else(|err| {
                    eprintln!("{}", err);
                })
        }
        WebhookSubcommand::Open(cmd_args) => {
            handle_open_environment_webhook(
                api_key,
                cmd.project,
                cmd.environment,
                cmd_args.webhook_id,
            )
            .await
            .unwrap_or_else(|err| {
                eprintln!("{}", err);
            });
        }
    }
}
