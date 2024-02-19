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
        update::{handle_update_webhook, UpdateWebhookArgs},
    },
};

pub async fn handle_webhook_commands(cmd: WebhookCommand, api_key: String, raw_output: bool) {
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
    }
}
