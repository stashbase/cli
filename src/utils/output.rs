use crate::cmd::config::OutputFormat;

pub fn get_output_format(
    raw_output: bool,
    default_output_format: Option<OutputFormat>,
    cmd_format: Option<OutputFormat>,
) -> OutputFormat {
    match raw_output {
        true => OutputFormat::Json,
        false => cmd_format.unwrap_or(default_output_format.unwrap_or_default()),
    }
}

pub fn get_is_json_output_format(
    raw_output: bool,
    default_output_format: Option<OutputFormat>,
) -> bool {
    match raw_output {
        true => true,
        false => match default_output_format == Some(OutputFormat::Json) {
            true => true,
            false => false,
        },
    }
}
pub fn write_indented(f: &mut std::fmt::Formatter<'_>, indent: usize, s: &str) -> std::fmt::Result {
    let indent_str = " ".repeat(indent);
    for line in s.lines() {
        writeln!(f, "{}{}", indent_str, line)?;
    }

    Ok(())
}
