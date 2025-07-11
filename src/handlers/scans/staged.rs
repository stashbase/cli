use crate::{
    api,
    models::{
        api_client::{GenericOutputError, OutputError, RequestApiOptionResponse},
        scans::{
            DiffHunk, DiffProcessingState, FileChangesScanResponse, FileHunks, ScanConfig,
            ScanFileChangesPayload,
        },
        validation::{InputValidationError, ScanInputValidationError},
    },
    utils::scans::{
        file_content_equals, filter_new_findings, filter_sha256_hashes, get_latest_scan_file,
        is_binary_file, load_baseline_results, process_diff_line, save_scan_results,
        should_exclude_file, SCAN_CONTEXT_LINES, SCAN_IGNORE_LINE_COMMENT,
    },
};
use colored_json::to_colored_json_auto;
use git2::Repository;
use spinoff::{spinners, Color, Spinner, Streams};
use std::{cell::RefCell, io::IsTerminal, path::Path, rc::Rc};

pub struct HandleScanStagedFileHunksArgs {
    pub api_key: String,
    pub json_format: bool,
    //
    pub exclude: Vec<String>,
    pub baseline: Option<String>,
    pub output_dir: Option<String>,
    pub config_file_path: Option<String>,
    pub ignore_value_hashes: Vec<String>,
}

pub async fn handle_scan_staged_file_hunks(
    args: HandleScanStagedFileHunksArgs,
) -> Result<(), anyhow::Error> {
    let HandleScanStagedFileHunksArgs {
        api_key,
        json_format,
        baseline,
        output_dir,
        config_file_path,
        ignore_value_hashes: _,
        exclude: _,
    } = args;

    let config = match &config_file_path {
        Some(path) => ScanConfig::load_from_file(path).unwrap_or_default(),
        None => ScanConfig::default(),
    };

    let exclude = config
        .exclude
        .into_iter()
        .flatten()
        .chain(args.exclude.into_iter())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();

    let staged_files_result = get_staged_file_hunks(
        SCAN_CONTEXT_LINES,
        &SCAN_IGNORE_LINE_COMMENT,
        &exclude,
        config_file_path.as_deref(),
    );

    if let Err(e) = staged_files_result {
        let input_validation_error = InputValidationError::Scan(e);
        let error_output = input_validation_error.format_error_output(json_format)?;

        eprintln!("\n{}", error_output);
        std::process::exit(1);
    }

    let staged_files = staged_files_result.unwrap();

    if staged_files.is_empty() {
        if json_format {
            let message = serde_json::json!({
                "message": "No staged changes to scan."
            });
            eprintln!("\n{}", to_colored_json_auto(&message).unwrap());
        } else {
            eprintln!("\nNo staged changes to scan.");
        }
        std::process::exit(0);
    }

    let ignore_value_hashes = {
        let hashes = config
            .ignore_value_hashes
            .into_iter()
            .flatten()
            .chain(args.ignore_value_hashes.into_iter())
            .flat_map(|hash| filter_sha256_hashes(vec![hash]))
            .collect::<std::collections::HashSet<_>>();

        if hashes.is_empty() {
            None
        } else {
            let sorted = hashes.into_iter().collect::<Vec<_>>();
            let mut sorted_hashes = sorted.clone();
            sorted_hashes.sort();

            Some(sorted_hashes)
        }
    };

    let data = ScanFileChangesPayload {
        ignore_value_hashes,
        files: staged_files,
    };

    let mut spinner = Spinner::new_with_stream(
        spinners::Dots,
        "Scanning staged changes...",
        Color::Cyan,
        Streams::Stderr,
    );

    let response = api::scans::scan_file_changes(api_key, &data).await;

    if let Err(err) = response {
        spinner.stop_and_persist("", "");

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

                        spinner.stop_and_persist("", "");

                        // Apply baseline filtering if baseline is provided
                        let filtered_data = if let Some(baseline_path) = baseline {
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

                        output_scan_findings(filtered_data, json_format, output_dir);

                        if is_empty {
                            std::process::exit(0);
                        } else {
                            std::process::exit(1);
                        }
                    }
                    Err(_) => {
                        spinner.stop_and_persist("", "");
                        let error = OutputError::failed_to_deserialize_response_body();
                        let formatted_err = error.format_error_output(json_format)?;

                        eprintln!("{}", formatted_err);
                        std::process::exit(1);
                    }
                }
            }
            None => {
                spinner.stop_and_persist("", "");

                match json_format {
                    true => {
                        let error = OutputError::Generic(GenericOutputError {
                            message: "Something went wrong.".to_string(),
                            code: None,
                            hint: Some("Please try again later.".to_string()),
                        });
                        let json_value = error.to_json_value().unwrap();

                        if std::io::stdout().is_terminal() {
                            eprintln!("\n{}", to_colored_json_auto(&json_value).unwrap());
                        } else {
                            eprintln!("{}", serde_json::to_string_pretty(&json_value).unwrap());
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
            spinner.stop_and_persist("", "");

            let error_output = e.format_error_output(json_format)?;
            eprintln!("{}", error_output);
            std::process::exit(1);
        }
    }
}

fn output_scan_findings(
    response: FileChangesScanResponse,
    json_format: bool,
    output_dir: Option<String>,
) {
    let is_empty = response.findings.is_empty();

    let json_value = serde_json::to_value(&response).unwrap();
    let pretty_json = serde_json::to_string_pretty(&json_value).unwrap();

    if json_format {
        if let Some(output_dir) = output_dir {
            if is_empty {
                let message = serde_json::json!({
                    "message": "No secrets detected in staged changes!"
                });
                eprintln!("{}", to_colored_json_auto(&message).unwrap());
            } else {
                let latest_file = get_latest_scan_file(&output_dir);

                match latest_file {
                    Some(file) => {
                        let file_path = file.path().to_string_lossy().to_string();
                        let content_equals = file_content_equals(&file_path, &pretty_json);

                        if content_equals {
                            let message = serde_json::json!({
                                "message": "Potential secrets detected in staged changes, results match previous scan.",
                                "file_path": file_path
                            });

                            eprintln!("{}", to_colored_json_auto(&message).unwrap());
                        } else {
                            let file_path = save_scan_results(&output_dir, &pretty_json);

                            let message = serde_json::json!({
                                "message": "Potential secrets detected in staged changes. Scan results saved to file.",
                                "file_path": file_path
                            });
                            eprintln!("{}", to_colored_json_auto(&message).unwrap());
                        }
                    }
                    None => {
                        let file_path = save_scan_results(&output_dir, &pretty_json);

                        let message = serde_json::json!({
                            "message": "Potential secrets detected in staged changes. Scan results saved to file.",
                            "file_path": file_path
                        });
                        eprintln!("{}", to_colored_json_auto(&message).unwrap());
                        //
                    }
                }
            }
        } else {
            if std::io::stdout().is_terminal() {
                let colored_json = to_colored_json_auto(&json_value).unwrap();
                println!("{}", colored_json);
            } else {
                println!("{}", pretty_json);
            }
        }
    } else {
        if is_empty {
            eprintln!("No secrets detected in staged changes!");
        } else {
            if let Some(output_dir) = output_dir {
                let latest_file = get_latest_scan_file(&output_dir);

                match latest_file {
                    Some(file) => {
                        let file_path = file.path().to_string_lossy().to_string();
                        let content_equals = file_content_equals(&file_path, &pretty_json);

                        if content_equals {
                            eprintln!("Potential secrets detected in staged changes, results match previous scan. File path: {}", file_path);
                        } else {
                            let file_path = save_scan_results(&output_dir, &pretty_json);
                            eprintln!("Potential secrets detected in your changes. Scan results saved to: {}", file_path);
                        }
                    }
                    None => {
                        let file_path = save_scan_results(&output_dir, &pretty_json);
                        eprintln!(
                            "Potential secrets detected in your changes. Scan results saved to: {}",
                            file_path
                        );
                        //
                    }
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

                    print!("{}", findings_string);
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
}

pub fn get_staged_file_hunks(
    context_lines: usize,
    ignore_line_comment: &str,
    exclude_patterns: &[String],
    config_file_path: Option<&str>,
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

    let diff = repo
        .diff_tree_to_index(Some(&head_tree), Some(&index), Some(&mut diff_opts))
        .map_err(|e| ScanInputValidationError::GitDiffGeneration {
            message: e.message().to_string(),
        })?;

    let state = Rc::new(RefCell::new(DiffProcessingState::new()));

    let ignore_line_comment = ignore_line_comment.to_string();

    {
        let state_ref = Rc::clone(&state);
        let mut file_callback = |delta: git2::DiffDelta, _progress: f32| {
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
                        let is_excluded = should_exclude_file(&file_path, &exclude_patterns);
                        state.excluded_files.insert(file_path.clone(), is_excluded);
                        is_excluded
                    }
                };

                if should_exclude {
                    return true;
                }

                let is_new_file = delta.status() == git2::Delta::Added;
                state.new_files.insert(file_path.clone(), is_new_file);

                if is_new_file {
                    let hunks = state
                        .files_with_hunks
                        .entry(file_path)
                        .or_insert_with(Vec::new);

                    let hunk = DiffHunk {
                        full_content: String::new(),
                        changes: None,
                        context_end_line: 2,
                        context_start_line: 1,
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
                    context_start_line: hunk.new_start() as usize,
                    context_end_line: (hunk.new_start() + hunk.new_lines()) as usize,
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
                                line_number >= hunk.context_start_line
                                    && line_number <= hunk.context_end_line
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

    let mut sorted = result.into_iter().collect::<Vec<_>>();
    sorted.sort_by_key(|file| file.file_path.clone());

    Ok(sorted)
}
