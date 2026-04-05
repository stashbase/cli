use clap::Args;

#[derive(Debug, Args)]
#[command(override_usage = "doctor [OPTIONS]")]
pub struct DoctorCommand {
    /// Perform API authentication check when API key is available
    #[arg(long = "auth-check")]
    pub auth_check: bool,
}
