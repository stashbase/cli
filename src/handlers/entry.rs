use crate::cmd::{
    projects::ProjectSubcommand,
    root::{Cli, EntityType},
};

pub fn handle_cli(args: Cli) {
    match args.entity_type {
        EntityType::Project(cmd) => match cmd.subcommand {
            ProjectSubcommand::List(_) => {
                todo!("List projects")
            }
        },
    }
}
