use crate::{
    api,
    handlers::scans::config::resolve_scan_config_path,
    models::{
        api_client::{GenericOutputError, OutputError, RequestApiOptionResponse},
        scans::{
            DiffHunk, DiffProcessingState, FileChangesScanResponse, FileHunks,
            IgnoredSecretsPayload, MatchConfigPayload, ProjectContextConfigPayload, ScanConfig,
            ScanFileChangesPayload, ScanOutputJson,
        },
        validation::{InputValidationError, ScanInputValidationError},
    },
    utils::{
        output::get_formatted_json_string,
        scans::{
            default_scan_exclude_patterns, file_content_equals, filter_new_findings,
            get_file_matches, get_latest_scan_file, is_binary_file, is_valid_sha256_hash,
            load_baseline_results, process_diff_line, save_scan_results, should_exclude_file,
            update_findings_with_file_matches, SCAN_CONTEXT_LINES, SCAN_IGNORE_LINE_COMMENT,
        },
        validation::validate_project_identifier,
    },
};
use anyhow::Result;
use git2::Repository;
use regex::Regex;
use spinoff::{spinners, Color, Spinner, Streams};
use std::{cell::RefCell, collections::HashMap, fs, io::IsTerminal, path::Path, rc::Rc};

pub struct HandleScanStagedFileHunksArgs {
    pub api_key: String,
    pub silent: bool,
    pub json_format: bool,
    //
    pub excluded_files: Vec<String>,
    pub baseline: Option<String>,
    pub output_dir: Option<String>,
    pub config_file_path: Option<String>,
    pub ignore_secret_hashes: Vec<String>,
    pub ignore_secret_regexes: Vec<String>,
    pub match_project: Option<String>,
    pub match_environments: Vec<String>,
    pub match_files: Vec<String>,
}

#[derive(Clone, Copy)]
enum FileScanMode {
    Staged,
    Changed,
}

impl FileScanMode {
    fn no_changes_message(self) -> &'static str {
        match self {
            FileScanMode::Staged => "No staged changes to scan.",
            FileScanMode::Changed => "No changed files to scan.",
        }
    }

    fn scanning_message(self) -> &'static str {
        match self {
            FileScanMode::Staged => "Scanning staged changes...",
            FileScanMode::Changed => "Scanning changed files...",
        }
    }

    fn no_secrets_message(self) -> &'static str {
        match self {
            FileScanMode::Staged => "No secrets detected in staged changes!",
            FileScanMode::Changed => "No secrets detected in changed files!",
        }
    }

    fn findings_match_message(self) -> &'static str {
        match self {
            FileScanMode::Staged => {
                "Potential secrets detected in staged changes, findings match previous scan."
            }
            FileScanMode::Changed => {
                "Potential secrets detected in changed files, findings match previous scan."
            }
        }
    }

    fn findings_saved_message(self) -> &'static str {
        match self {
            FileScanMode::Staged => {
                "Potential secrets detected in staged changes. Scan findings saved to file."
            }
            FileScanMode::Changed => {
                "Potential secrets detected in changed files. Scan findings saved to file."
            }
        }
    }
}

pub async fn handle_scan_staged_file_hunks(
    args: HandleScanStagedFileHunksArgs,
) -> Result<(), anyhow::Error> {
    handle_scan_file_hunks(args, FileScanMode::Staged).await
}

pub async fn handle_scan_changed_file_hunks(
    args: HandleScanStagedFileHunksArgs,
) -> Result<(), anyhow::Error> {
    handle_scan_file_hunks(args, FileScanMode::Changed).await
}

async fn handle_scan_file_hunks(
    args: HandleScanStagedFileHunksArgs,
    mode: FileScanMode,
) -> Result<(), anyhow::Error> {
    let HandleScanStagedFileHunksArgs {
        api_key,
        silent,
        json_format,
        baseline,
        output_dir,
        config_file_path,
        ignore_secret_hashes,
        ignore_secret_regexes,
        match_project,
        match_environments,
        match_files,
        excluded_files,
    } = args;

    let config_file_path = resolve_scan_config_path(config_file_path);

    let config = match &config_file_path {
        Some(path) => match ScanConfig::load_from_file(path) {
            Ok(config) => config,
            Err(e) => {
                let error = InputValidationError::Scan(e);
                let error_output = error.format_error_output(json_format).unwrap();

                if !silent {
                    eprintln!("\n{}", error_output);
                } else {
                    eprintln!("{}", error_output);
                }

                std::process::exit(1);
            }
        },
        None => ScanConfig::default(),
    };

    let exclude = default_scan_exclude_patterns()
        .into_iter()
        .chain(config.excluded_files.into_iter().flatten())
        .chain(excluded_files.into_iter())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();

    let file_hunks_result = match mode {
        FileScanMode::Staged => get_staged_file_hunks(
            SCAN_CONTEXT_LINES,
            &SCAN_IGNORE_LINE_COMMENT,
            &exclude,
            config_file_path.as_deref(),
        ),
        FileScanMode::Changed => get_changed_file_hunks(
            SCAN_CONTEXT_LINES,
            &SCAN_IGNORE_LINE_COMMENT,
            &exclude,
            config_file_path.as_deref(),
        ),
    };

    if let Err(e) = file_hunks_result {
        let input_validation_error = InputValidationError::Scan(e);
        let error_output = input_validation_error.format_error_output(json_format)?;

        if !silent {
            eprintln!("\n{}", error_output);
        } else {
            eprintln!("{}", error_output);
        }

        std::process::exit(1);
    }

    let file_hunks = file_hunks_result.unwrap();

    if file_hunks.is_empty() {
        if !silent {
            if json_format {
                let message = serde_json::json!({
                    "message": mode.no_changes_message()
                });
                let pretty = get_formatted_json_string(&message, false).unwrap();
                println!("\n{}", pretty);
            } else {
                println!("{}", mode.no_changes_message());
            }
        }

        std::process::exit(0);
    }

    let mut ignored_secrets_payload = IgnoredSecretsPayload::default();

    // regexes
    let all_regexes = ignore_secret_regexes
        .into_iter()
        .chain(
            config
                .ignored_secrets
                .as_ref()
                .and_then(|c| c.regexes.as_ref())
                .into_iter()
                .flatten()
                .cloned(),
        )
        .collect::<Vec<_>>();

    if !all_regexes.is_empty() {
        let validate_and_dedupe_regexes =
            |regexes: Vec<String>| -> Result<Vec<String>, (String, String)> {
                let mut seen = std::collections::HashSet::new();
                let mut unique = Vec::new();
                for regex in regexes {
                    if !seen.insert(regex.clone()) {
                        continue;
                    }
                    if let Err(e) = Regex::new(&regex) {
                        return Err((regex, e.to_string()));
                    }
                    unique.push(regex);
                }
                Ok(unique)
            };

        match validate_and_dedupe_regexes(all_regexes) {
            Ok(unique_valid_regexes) => {
                ignored_secrets_payload.regexes = unique_valid_regexes;
            }
            Err((regex, message)) => {
                let scan_error =
                    ScanInputValidationError::InvalidIgnoreSecretRegex { regex, message };

                let input_validation_error = InputValidationError::Scan(scan_error);
                let error_output = input_validation_error.format_error_output(json_format)?;

                if !silent {
                    eprintln!("\n{}", error_output);
                } else {
                    eprintln!("{}", error_output);
                }

                std::process::exit(1);
            }
        }
    }

    // hashes
    let all_hashes = ignore_secret_hashes
        .into_iter()
        .chain(
            config
                .ignored_secrets
                .as_ref()
                .and_then(|c| c.hashes.as_ref())
                .into_iter()
                .flatten()
                .cloned(),
        )
        .collect::<Vec<_>>();

    if !all_hashes.is_empty() {
        // validate hashes
        if let Some(invalid_hash) = all_hashes.iter().find(|hash| !is_valid_sha256_hash(hash)) {
            let scan_error = ScanInputValidationError::InvalidIgnoreSecretHash {
                hash: invalid_hash.clone(),
            };
            let input_validation_error = InputValidationError::Scan(scan_error);
            let error_output = input_validation_error.format_error_output(json_format)?;

            if !silent {
                eprintln!("\n{}", error_output);
            } else {
                eprintln!("{}", error_output);
            }

            std::process::exit(1);
        }

        let sorted = all_hashes.into_iter().collect::<Vec<_>>();
        let mut sorted_hashes = sorted.clone();
        sorted_hashes.sort();

        ignored_secrets_payload.hashes = sorted_hashes;
    }

    let match_config = config.match_config.clone();

    let all_environments = match_environments
        .into_iter()
        .chain(
            match_config
                .clone()
                .as_ref()
                .and_then(|c| c.project.as_ref().and_then(|p| p.environments.as_ref()))
                .into_iter()
                .flatten()
                .cloned(),
        )
        .collect::<Vec<_>>();

    let unique_environments = all_environments
        .into_iter()
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();

    let project_identifier = match match_project {
        Some(p) => Some(p),
        None => match config.match_config {
            Some(c) => c.project.as_ref().and_then(|p| p.identifier.clone()),
            None => None,
        },
    };

    if unique_environments.len() > 0 && project_identifier.is_none() {
        let error = ScanInputValidationError::MissingMatchProjectIdentifier;
        let input_error = InputValidationError::Scan(error);
        let error_output = input_error.format_error_output(json_format)?;

        if !silent {
            eprintln!("\n{}", error_output);
        } else {
            eprintln!("{}", error_output);
        }

        std::process::exit(1);
    }

    let mut project_context_config: Option<ProjectContextConfigPayload> = None;

    if let Some(project) = project_identifier {
        if !project.trim().is_empty() {
            let identifier_is_valid = validate_project_identifier(&project, true);

            if let Err(err) = identifier_is_valid {
                let error_output = err.format_error_output(json_format)?;
                if !silent {
                    eprintln!("\n{}", error_output);
                } else {
                    eprintln!("{}", error_output);
                }

                std::process::exit(1);
            } else {
                let envs = match unique_environments.is_empty() {
                    true => None,
                    false => Some(unique_environments),
                };

                project_context_config = Some(ProjectContextConfigPayload {
                    identifier: project,
                    environments: envs,
                });
            }
        }
    }

    let data = ScanFileChangesPayload {
        files: file_hunks,
        match_config: match project_context_config {
            Some(c) => Some(MatchConfigPayload { project: Some(c) }),
            None => None,
        },
        ignored_secrets: match ignored_secrets_payload.is_empty() {
            true => None,
            false => Some(ignored_secrets_payload),
        },
    };

    let spinner = if !silent {
        Some(Spinner::new_with_stream(
            spinners::Dots,
            mode.scanning_message(),
            Color::Cyan,
            Streams::Stderr,
        ))
    } else {
        None
    };

    let response = api::scans::scan_file_changes(api_key, &data).await;

    if let Err(err) = response {
        if let Some(mut spinner) = spinner {
            spinner.stop_and_persist("", "");
        }

        let error_output = err.format_error_output(json_format)?;
        eprintln!("{}", error_output);
        std::process::exit(1);
    }

    let response = response.unwrap();

    match response {
        RequestApiOptionResponse::Ok(res) => match res.text {
            Some(text) => {
                let response = serde_json::from_str::<FileChangesScanResponse>(&text);

                match response {
                    Ok(data) => {
                        let output_dir = match output_dir {
                            Some(dir) => Some(dir),
                            None => config.output_dir,
                        };

                        if let Some(mut spinner) = spinner {
                            spinner.stop_and_persist("", "");
                        }

                        // Apply baseline filtering if baseline is provided
                        let mut filtered_data = if let Some(baseline_path) = baseline {
                            match load_baseline_results(&baseline_path) {
                                Ok(baseline_results) => {
                                    let filtered_findings =
                                        filter_new_findings(data.findings, baseline_results);
                                    FileChangesScanResponse {
                                        skipped_files: data.skipped_files,
                                        findings: filtered_findings,
                                    }
                                }
                                Err(e) => {
                                    let input_validation_error = InputValidationError::Scan(e);
                                    let error_output =
                                        input_validation_error.format_error_output(json_format)?;

                                    eprintln!("{}", error_output);
                                    std::process::exit(1);
                                }
                            }
                        } else {
                            data
                        };

                        let is_empty = filtered_data.findings.is_empty();

                        if !is_empty {
                            let all_files_to_match = match_files
                                .into_iter()
                                .chain(
                                    match_config
                                        .as_ref()
                                        .and_then(|ref c| c.files.as_ref())
                                        .into_iter()
                                        .flatten()
                                        .cloned(),
                                )
                                .collect::<Vec<_>>();

                            let file_matches = match all_files_to_match.is_empty() {
                                true => HashMap::new(),
                                false => get_file_matches(
                                    all_files_to_match,
                                    filtered_data.findings.clone(),
                                ),
                            };

                            // Update findings with matched secrets from files
                            if !file_matches.is_empty() {
                                update_findings_with_file_matches(
                                    &mut filtered_data.findings,
                                    file_matches,
                                );
                            }
                        }

                        let output_res =
                            output_scan_findings(filtered_data, json_format, &output_dir, mode);

                        if let Err(e) = output_res {
                            let scan_error = ScanInputValidationError::FailedToSaveScanResults {
                                output_dir: output_dir.unwrap_or(String::new()),
                                message: e.to_string(),
                            };

                            let error = InputValidationError::Scan(scan_error);
                            let error_output = error.format_error_output(json_format).unwrap();

                            eprintln!("{}", error_output);
                            std::process::exit(1);
                        }

                        if is_empty {
                            std::process::exit(0);
                        } else {
                            std::process::exit(1);
                        }
                    }
                    Err(_) => {
                        if let Some(mut spinner) = spinner {
                            spinner.stop_and_persist("", "");
                        }

                        let error = OutputError::failed_to_deserialize_response_body();
                        let formatted_err = error.format_error_output(json_format)?;

                        eprintln!("{}", formatted_err);
                        std::process::exit(1);
                    }
                }
            }
            None => {
                if let Some(mut spinner) = spinner {
                    spinner.stop_and_persist("", "");
                }

                match json_format {
                    true => {
                        let error = OutputError::Generic(GenericOutputError {
                            message: "Something went wrong.".to_string(),
                            code: None,
                            status: None,
                            hint: Some("Please try again later.".to_string()),
                            details: None,
                        });

                        let pretty = get_formatted_json_string(&error, false).unwrap();

                        if !silent {
                            eprintln!("\n{}", pretty);
                        } else {
                            eprintln!("{}", pretty);
                        }
                    }
                    false => {
                        eprintln!("Something went wrong.");
                    }
                }

                std::process::exit(1);
            }
        },
        RequestApiOptionResponse::Err(e) => {
            if let Some(mut spinner) = spinner {
                spinner.stop_and_persist("", "");
            }

            let error_output = e.format_error_output(json_format)?;
            eprintln!("{}", error_output);
            std::process::exit(1);
        }
    }
}

fn output_scan_findings(
    response: FileChangesScanResponse,
    json_format: bool,
    output_dir: &Option<String>,
    mode: FileScanMode,
) -> Result<()> {
    let is_empty = response.findings.is_empty();

    let json_value = serde_json::to_value(&response).unwrap();
    let pretty_json = serde_json::to_string_pretty(&json_value).unwrap();

    if json_format {
        if let Some(output_dir) = output_dir {
            if is_empty {
                let message = ScanOutputJson {
                    message: mode.no_secrets_message().to_string(),
                    skipped_files: response.skipped_files,
                    skipped_commits: None,
                    file_path: None,
                };

                let pretty = message.to_json_pretty(false).unwrap();
                eprintln!("{}", pretty);
            } else {
                let latest_file = get_latest_scan_file(output_dir);

                match latest_file {
                    Some(file) => {
                        let file_path = file.path().to_string_lossy().to_string();
                        let content_equals = file_content_equals(&file_path, &pretty_json);

                        if content_equals {
                            let message = ScanOutputJson {
                                message: mode.findings_match_message().to_string(),
                                file_path: Some(file_path),
                                skipped_files: response.skipped_files,
                                skipped_commits: None,
                            };

                            let pretty = message.to_json_pretty(false).unwrap();
                            eprintln!("{}", pretty);
                        } else {
                            let file_path = save_scan_results(output_dir, &pretty_json)?;
                            let message = ScanOutputJson {
                                message: mode.findings_saved_message().to_string(),
                                file_path: Some(file_path),
                                skipped_files: response.skipped_files,
                                skipped_commits: None,
                            };

                            let pretty = message.to_json_pretty(false).unwrap();
                            eprintln!("{}", pretty);
                        }
                    }
                    None => {
                        let file_path = save_scan_results(output_dir, &pretty_json)?;

                        let message = ScanOutputJson {
                            message: mode.findings_saved_message().to_string(),
                            file_path: Some(file_path),
                            skipped_files: response.skipped_files,
                            skipped_commits: None,
                        };

                        let pretty = message.to_json_pretty(false).unwrap();
                        eprintln!("{}", pretty);
                    }
                }
            }
        } else {
            let pretty = get_formatted_json_string(&json_value, false).unwrap();
            println!("{}", pretty);
        }
    } else {
        if is_empty {
            eprintln!("{}", mode.no_secrets_message());
        } else {
            if let Some(output_dir) = output_dir {
                let latest_file = get_latest_scan_file(output_dir);

                match latest_file {
                    Some(file) => {
                        let file_path = file.path().to_string_lossy().to_string();
                        let content_equals = file_content_equals(&file_path, &pretty_json);

                        if content_equals {
                            eprintln!("{} File path: {}", mode.findings_match_message(), file_path);
                        } else {
                            let file_path = save_scan_results(output_dir, &pretty_json)?;
                            eprintln!("Potential secrets detected in your changes. Scan findings saved to: {}", file_path);
                        }
                    }
                    None => {
                        let file_path = save_scan_results(output_dir, &pretty_json)?;

                        eprintln!(
                            "Potential secrets detected in your changes. Scan findings saved to: {}",
                            file_path
                        );
                    }
                }

                if let Some(skipped_files) = &response.skipped_files {
                    eprintln!("Skipped files: {}", skipped_files.join(", "));
                }
            } else {
                eprintln!("Potential secrets detected in your changes, please review the findings before committing:");
                if let Some(skipped_files) = &response.skipped_files {
                    eprintln!("Skipped files: {}", skipped_files.join(", "));
                }
                eprintln!();

                if std::io::stdout().is_terminal() {
                    let findings_string = response
                        .findings
                        .iter()
                        .map(|result| result.get_colored_string())
                        .collect::<Vec<_>>()
                        .join("\n\n");

                    println!("{}", findings_string);
                } else {
                    let findings_string = response
                        .findings
                        .iter()
                        .map(|result| format!("{}", result))
                        .collect::<Vec<_>>()
                        .join("\n\n");

                    println!("{}", findings_string);
                }
            }
        }
    }

    Ok(())
}

pub fn get_staged_file_hunks(
    context_lines: usize,
    ignore_line_comment: &str,
    exclude_patterns: &[String],
    config_file_path: Option<&str>,
) -> Result<Vec<FileHunks>, ScanInputValidationError> {
    get_file_hunks(
        context_lines,
        ignore_line_comment,
        exclude_patterns,
        config_file_path,
        FileScanMode::Staged,
    )
}

pub fn get_changed_file_hunks(
    context_lines: usize,
    ignore_line_comment: &str,
    exclude_patterns: &[String],
    config_file_path: Option<&str>,
) -> Result<Vec<FileHunks>, ScanInputValidationError> {
    get_file_hunks(
        context_lines,
        ignore_line_comment,
        exclude_patterns,
        config_file_path,
        FileScanMode::Changed,
    )
}

fn get_file_hunks(
    context_lines: usize,
    ignore_line_comment: &str,
    exclude_patterns: &[String],
    config_file_path: Option<&str>,
    mode: FileScanMode,
) -> Result<Vec<FileHunks>, ScanInputValidationError> {
    let repo = Repository::open(".").map_err(|e| {
        if e.code() == git2::ErrorCode::NotFound {
            ScanInputValidationError::GitRepositoryNotFound
        } else {
            ScanInputValidationError::GitRepositoryAccess {
                message: e.message().to_string(),
            }
        }
    })?;

    let repo_for_head = Repository::open(".").map_err(|e| {
        if e.code() == git2::ErrorCode::NotFound {
            ScanInputValidationError::GitRepositoryNotFound
        } else {
            ScanInputValidationError::GitRepositoryAccess {
                message: e.message().to_string(),
            }
        }
    })?;

    let index = repo
        .index()
        .map_err(|e| ScanInputValidationError::GitIndexAccess {
            message: e.message().to_string(),
        })?;

    let head_tree = match repo_for_head.head() {
        Ok(head) => head
            .peel_to_tree()
            .map_err(|e| ScanInputValidationError::GitTreeAccess {
                message: e.message().to_string(),
            })?,
        Err(_) => {
            let empty_tree = repo_for_head.treebuilder(None).map_err(|e| {
                ScanInputValidationError::GitTreeAccess {
                    message: e.message().to_string(),
                }
            })?;
            let oid = empty_tree
                .write()
                .map_err(|e| ScanInputValidationError::GitTreeAccess {
                    message: e.message().to_string(),
                })?;
            repo_for_head
                .find_tree(oid)
                .map_err(|e| ScanInputValidationError::GitTreeAccess {
                    message: e.message().to_string(),
                })?
        }
    };

    let mut diff_opts = git2::DiffOptions::new();
    diff_opts.context_lines(context_lines as u32);
    diff_opts.show_binary(false);
    if matches!(mode, FileScanMode::Changed) {
        diff_opts.include_untracked(true);
        diff_opts.recurse_untracked_dirs(true);
    }

    let diff = match mode {
        FileScanMode::Staged => {
            repo.diff_tree_to_index(Some(&head_tree), Some(&index), Some(&mut diff_opts))
        }
        FileScanMode::Changed => {
            repo.diff_tree_to_workdir_with_index(Some(&head_tree), Some(&mut diff_opts))
        }
    }
    .map_err(|e| ScanInputValidationError::GitDiffGeneration {
        message: e.message().to_string(),
    })?;

    let state = Rc::new(RefCell::new(DiffProcessingState::new()));

    let ignore_line_comment = ignore_line_comment.to_string();

    {
        let state_ref = Rc::clone(&state);
        let mut file_callback = |delta: git2::DiffDelta, _progress: f32| {
            // Skip deleted files - we don't want to scan files that were removed
            if delta.status() == git2::Delta::Deleted {
                return true;
            }

            if let Some(new_file) = delta.new_file().path() {
                let file_path = new_file.to_string_lossy().to_string();

                let extension = Path::new(&file_path)
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .unwrap_or("");

                let is_binary = is_binary_file(extension);

                if is_binary {
                    return true;
                }

                if let Some(config_file_path) = config_file_path {
                    if file_path == config_file_path {
                        return true;
                    }
                }

                let mut state = state_ref.borrow_mut();
                let should_exclude = {
                    if let Some(&is_excluded) = state.excluded_files.get(&file_path) {
                        is_excluded
                    } else {
                        let is_excluded_res = should_exclude_file(&file_path, &exclude_patterns);

                        match is_excluded_res {
                            Ok(is_excluded) => {
                                state.excluded_files.insert(file_path.clone(), is_excluded);
                                is_excluded
                            }
                            Err(_) => false,
                        }
                    }
                };

                if should_exclude {
                    return true;
                }

                let is_new_file = delta.status() == git2::Delta::Added
                    || delta.status() == git2::Delta::Untracked;
                state.new_files.insert(file_path.clone(), is_new_file);

                if is_new_file {
                    let hunks = state
                        .files_with_hunks
                        .entry(file_path)
                        .or_insert_with(Vec::new);

                    let hunk = DiffHunk {
                        full_content: String::new(),
                        changes: None,
                        start_line: 1,
                        end_line: 2,
                    };

                    hunks.push(hunk);
                } else {
                    state
                        .files_with_hunks
                        .entry(file_path)
                        .or_insert_with(Vec::new);
                }
            }
            true
        };

        let state_hunk = Rc::clone(&state);
        let mut hunk_callback = move |delta: git2::DiffDelta, hunk: git2::DiffHunk| {
            // Skip deleted files
            if delta.status() == git2::Delta::Deleted {
                return true;
            }

            if let Some(new_file) = delta.new_file().path() {
                let file_path = new_file.to_string_lossy().to_string();

                if let Some(config_file_path) = config_file_path {
                    if file_path == config_file_path {
                        return true;
                    }
                }

                let state = state_hunk.borrow();
                if state
                    .excluded_files
                    .get(&file_path)
                    .copied()
                    .unwrap_or(false)
                {
                    return true;
                }

                if state.new_files.get(&file_path).copied().unwrap_or(false) {
                    return true;
                }
                drop(state);

                let hunk_with_context = DiffHunk {
                    full_content: String::new(),
                    changes: Some(Vec::new()),
                    start_line: hunk.new_start() as usize,
                    end_line: (hunk.new_start() + hunk.new_lines()) as usize,
                };

                state_hunk
                    .borrow_mut()
                    .files_with_hunks
                    .entry(file_path)
                    .or_insert_with(Vec::new)
                    .push(hunk_with_context);
            }
            true
        };

        let state_line = Rc::clone(&state);
        let ignore_line_comment_clone = ignore_line_comment.clone();

        let mut line_callback =
            move |delta: git2::DiffDelta, _hunk: Option<git2::DiffHunk>, line: git2::DiffLine| {
                // Skip deleted files
                if delta.status() == git2::Delta::Deleted {
                    return true;
                }

                if let Some(new_file) = delta.new_file().path() {
                    let file_path = new_file.to_string_lossy().to_string();

                    let state = state_line.borrow();
                    if state
                        .excluded_files
                        .get(&file_path)
                        .copied()
                        .unwrap_or(false)
                    {
                        return true;
                    }

                    let is_new_file = state.new_files.get(&file_path).copied().unwrap_or(false);
                    drop(state);

                    let mut state = state_line.borrow_mut();
                    if let Some(hunks) = state.files_with_hunks.get_mut(&file_path) {
                        let line_number =
                            line.new_lineno().unwrap_or(line.old_lineno().unwrap_or(0)) as usize;

                        let target_hunk_index = if is_new_file {
                            Some(0)
                        } else {
                            hunks.iter().position(|hunk| {
                                line_number >= hunk.start_line && line_number <= hunk.end_line
                            })
                        };

                        if let Some(hunk_index) = target_hunk_index {
                            let hunk_key = (file_path.clone(), hunk_index);

                            // Drop the mutable borrow of state to get values
                            drop(state);
                            let mut state = state_line.borrow_mut();

                            let mut current_change = state
                                .current_changes
                                .get(&hunk_key)
                                .cloned()
                                .unwrap_or(None);
                            let mut prev_line = state.prev_line.clone();

                            // Get a new mutable borrow for hunks
                            if let Some(hunks) = state.files_with_hunks.get_mut(&file_path) {
                                if let Some(target_hunk) = hunks.get_mut(hunk_index) {
                                    process_diff_line(
                                        line,
                                        &file_path,
                                        is_new_file,
                                        &mut current_change,
                                        target_hunk,
                                        &mut prev_line,
                                        &ignore_line_comment_clone,
                                        context_lines,
                                    );

                                    state.prev_line = prev_line;
                                    state.current_changes.insert(hunk_key, current_change);
                                }
                            }
                        }
                    }
                }
                true
            };

        diff.foreach(
            &mut file_callback,
            None,
            Some(&mut hunk_callback),
            Some(&mut line_callback),
        )
        .map_err(|e| ScanInputValidationError::GitDiffProcessing {
            message: e.message().to_string(),
        })?;

        let mut state = state.borrow_mut();
        let current_changes: Vec<_> = state
            .current_changes
            .iter()
            .filter_map(|((file_path, hunk_index), change_opt)| {
                change_opt
                    .as_ref()
                    .map(|change| (file_path.clone(), *hunk_index, change.clone()))
            })
            .collect();

        for (file_path, hunk_index, change) in current_changes {
            if let Some(hunks) = state.files_with_hunks.get_mut(&file_path) {
                if let Some(target_hunk) = hunks.get_mut(hunk_index) {
                    if let Some(changes) = &mut target_hunk.changes {
                        changes.push(change);
                    }
                }
            }
        }
    }

    let state = state.borrow();
    let result: Vec<FileHunks> = FileHunks::merge_overlapping_hunks(
        state
            .files_with_hunks
            .iter()
            .map(|(file_path, hunks)| FileHunks {
                file_path: file_path.clone(),
                hunks: hunks.clone(),
            })
            .collect(),
        context_lines,
    )
    .into_iter()
    .filter_map(|mut file| {
        // Filter hunks to only include non-empty ones
        file.hunks = file
            .hunks
            .into_iter()
            .filter(|hunk| {
                hunk.changes
                    .as_ref()
                    .map_or(true, |changes| !changes.is_empty())
            })
            .collect();

        // Only keep files that have non-empty hunks
        if file.hunks.is_empty() {
            None
        } else {
            Some(file)
        }
    })
    .collect();

    // Populate full_content for each hunk by reading from the selected source
    let mut result_with_full_content = result;
    for file_hunks in &mut result_with_full_content {
        let is_new_file = state
            .new_files
            .get(&file_hunks.file_path)
            .copied()
            .unwrap_or(false);

        let file_content = match mode {
            FileScanMode::Staged => {
                if let Some(index_entry) =
                    index.get_path(std::path::Path::new(&file_hunks.file_path), 0)
                {
                    if let Ok(blob) = repo.find_blob(index_entry.id) {
                        std::str::from_utf8(blob.content())
                            .ok()
                            .map(|s| s.to_string())
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            FileScanMode::Changed => fs::read_to_string(&file_hunks.file_path).ok(),
        };

        if let Some(file_content) = file_content {
            let lines: Vec<&str> = file_content.lines().collect();

            for hunk in &mut file_hunks.hunks {
                // For new files, read all lines from start_line to the end of file
                if is_new_file {
                    let start_idx = (hunk.start_line.saturating_sub(1)).min(lines.len());
                    let hunk_lines = &lines[start_idx..];
                    hunk.full_content = hunk_lines.join("\n");
                    if !hunk.full_content.is_empty() && file_content.ends_with('\n') {
                        hunk.full_content.push('\n');
                    }
                } else {
                    // For modified files, read the exact range
                    let start_idx = (hunk.start_line.saturating_sub(1)).min(lines.len());
                    let end_idx = hunk.end_line.min(lines.len());

                    if start_idx < end_idx {
                        let hunk_lines = &lines[start_idx..end_idx];
                        hunk.full_content = hunk_lines.join("\n");
                        // Add trailing newline if the original file has it and this isn't the last line
                        if !hunk.full_content.is_empty() && end_idx < lines.len() {
                            hunk.full_content.push('\n');
                        } else if end_idx == lines.len() && file_content.ends_with('\n') {
                            hunk.full_content.push('\n');
                        }
                    }
                }
            }
        }
    }

    let mut sorted = result_with_full_content.into_iter().collect::<Vec<_>>();
    sorted.sort_by_key(|file| file.file_path.clone());

    Ok(sorted)
}
