use crate::{
    cmd::projects::{ProjectCommands, ProjectSubcommand, ProjectsFromat},
    handlers::projects::{
        create::handle_create_project,
        delete::handle_delete_project,
        get::handle_get_project,
        list::{handle_list_projects, HandleListProjectsArgs},
        open::handle_open_project,
        update::handle_update_project,
    },
};

pub async fn handle_project_commands(cmd: ProjectCommands, api_key: String, raw_output: bool) {
    match cmd.subcommand {
        ProjectSubcommand::List(args) => {
            let args = HandleListProjectsArgs {
                api_key,
                search: args.search,
                sort: args.sort,
                descending: args.descending,
                format: match raw_output {
                    true => ProjectsFromat::Json,
                    false => args.format.unwrap_or_default(),
                },
            };

            handle_list_projects(args).await.unwrap_or_else(|err| {
                println!("{:?}", err);
            });
        }

        ProjectSubcommand::Get(args) => {
            handle_get_project(
                api_key,
                match raw_output {
                    true => ProjectsFromat::Json,
                    false => args.format.unwrap_or_default(),
                },
                args.name,
            )
            .await
            .unwrap_or_else(|err| {
                eprintln!("{:?}", err);
            });
        }

        ProjectSubcommand::Create(args) => {
            handle_create_project(api_key, args.name, args.description)
                .await
                .unwrap_or_else(|err| {
                    eprintln!("{:?}", err);
                });
        }

        ProjectSubcommand::Delete(args) => {
            handle_delete_project(api_key, args.name)
                .await
                .unwrap_or_else(|err| {
                    eprintln!("{:?}", err);
                });
        }
        ProjectSubcommand::Open(args) => {
            handle_open_project(api_key, args.name)
                .await
                .unwrap_or_else(|err| {
                    eprintln!("{:?}", err);
                });
        }

        ProjectSubcommand::Update(args) => {
            handle_update_project(api_key, args.name, args.new_name, args.description)
                .await
                .unwrap_or_else(|err| {
                    eprintln!("{:?}", err);
                });
        }
    }
}
