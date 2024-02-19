use crate::{
    cmd::{
        environments::EnvironmentFormat,
        webhooks::{WebhookCommand, WebhookSubcommand},
    },
    handlers::webhooks::list::{handle_list_webhooks, ListWebhooksArgs},
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
                eprintln!("{:?}", err);
            });
        }
        WebhookSubcommand::Get(_) => todo!(),
    }
}
