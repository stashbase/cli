use crate::{
    cmd::projects::{ProjectCommands, ProjectSubcommand},
    handlers::projects::{
        create::handle_create_project, delete::handle_delete_project, get::handle_get_project,
        list::handle_list_projects, open::handle_open_project, update::handle_update_project,
    },
};

pub async fn handle_project_commands(cmd: ProjectCommands, token: String, raw_output: bool) {
    match cmd.subcommand {
        ProjectSubcommand::List(_) => {
            handle_list_projects(token, raw_output)
                .await
                .unwrap_or_else(|err| {
                    println!("{:?}", err);
                });
        }

        ProjectSubcommand::Get(args) => {
            handle_get_project(token, raw_output, args.name)
                .await
                .unwrap_or_else(|err| {
                    eprintln!("{:?}", err);
                });
        }

        ProjectSubcommand::Create(args) => {
            handle_create_project(token, args.name, args.description)
                .await
                .unwrap_or_else(|err| {
                    eprintln!("{:?}", err);
                });
        }

        ProjectSubcommand::Delete(args) => {
            handle_delete_project(token, args.name)
                .await
                .unwrap_or_else(|err| {
                    eprintln!("{:?}", err);
                });
        }
        ProjectSubcommand::Open(args) => {
            handle_open_project(token, args.name)
                .await
                .unwrap_or_else(|err| {
                    eprintln!("{:?}", err);
                });
        }

        ProjectSubcommand::Update(args) => {
            handle_update_project(token, args.name, args.new_name, args.description)
                .await
                .unwrap_or_else(|err| {
                    eprintln!("{:?}", err);
                });
        }
    }
}
