use colored_json::to_colored_json_auto;
use git2::Repository;
use spinoff::{spinners, Color, Spinner, Streams};
use std::{cell::RefCell, io::IsTerminal, path::PathBuf, rc::Rc};

use crate::{
    api,
    models::{
        api_client::{GenericOutputError, OutputError, RequestApiOptionResponse},
        scans::{
            CommitChanges, CommitsScanResponse, DiffHunk, DiffProcessingState, FileHunks,
            ScanCommitChangesPayload, ScanConfig,
        },
        validation::{InputValidationError, ScanInputValidationError},
    },
    utils::scans::{
        file_content_equals, filter_new_findings, filter_sha256_hashes, get_latest_scan_file,
        is_binary_file, load_baseline_results, process_diff_line, save_scan_results,
        should_exclude_file, SCAN_CONTEXT_LINES, SCAN_IGNORE_LINE_COMMENT,
    },
};

pub struct HandleScanUnpushedCommitHunksArgs {
    pub api_key: String,
    pub json_format: bool,

    pub exclude: Vec<String>,
    pub baseline: Option<String>,
    pub output_dir: Option<String>,
    pub config_file_path: Option<String>,
    pub ignore_value_hashes: Vec<String>,
}

pub async fn handle_scan_unpushed_commit_hunks(
    args: HandleScanUnpushedCommitHunksArgs,
) -> Result<(), anyhow::Error> {
    let HandleScanUnpushedCommitHunksArgs {
        api_key,
        json_format,
        baseline,
        output_dir,
        config_file_path,
        ignore_value_hashes: _,
        exclude: _,
    } = args;

    let config = match &config_file_path {
        Some(path) => match ScanConfig::load_from_file(path) {
            Ok(config) => config,
            Err(e) => {
                let error = InputValidationError::Scan(e);
                let error_output = error.format_error_output(json_format).unwrap();
                eprintln!("\n{}", error_output);

                std::process::exit(1);
            }
        },
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

    let unpushed_commit_hunks_result = get_unpushed_commit_hunks(
        SCAN_CONTEXT_LINES,
        &SCAN_IGNORE_LINE_COMMENT,
        &exclude,
        config_file_path.as_deref(),
    );

    if let Err(e) = unpushed_commit_hunks_result {
        let input_validation_error = InputValidationError::Scan(e);
        let error_output = input_validation_error.format_error_output(json_format)?;

        eprintln!("\n{}", error_output);
        std::process::exit(1);
    }

    let unpushed_commit_hunks = unpushed_commit_hunks_result.unwrap();

    if unpushed_commit_hunks.is_empty() {
        if json_format {
            let message = serde_json::json!({
                "message": "No unpushed commits to scan."
            });
            eprintln!("\n{}", to_colored_json_auto(&message).unwrap());
        } else {
            eprintln!("\nNo unpushed commits to scan.");
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

    let data = ScanCommitChangesPayload {
        ignore_value_hashes,
        commits: unpushed_commit_hunks,
    };

    let mut spinner = Spinner::new_with_stream(
        spinners::Dots,
        "Scanning commits...",
        Color::Cyan,
        Streams::Stderr,
    );

    let response = api::scans::scan_commits(api_key, &data).await;

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
                let response = serde_json::from_str::<CommitsScanResponse>(&text);

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

                                    CommitsScanResponse {
                                        skipped_commits: data.skipped_commits,
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
    response: CommitsScanResponse,
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
                    "message": "No secrets detected in unpushed commits!"
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
                                "message": "Potential secrets detected in unpushed commits, results match previous scan.",
                                "file_path": file_path
                            });

                            eprintln!("{}", to_colored_json_auto(&message).unwrap());
                        } else {
                            let file_path = save_scan_results(&output_dir, &pretty_json);

                            let message = serde_json::json!({
                                "message": "Potential secrets detected in unpushed commits. Scan results saved to file.",
                                "file_path": file_path
                            });
                            eprintln!("{}", to_colored_json_auto(&message).unwrap());
                        }
                    }
                    None => {
                        let file_path = save_scan_results(&output_dir, &pretty_json);

                        let message = serde_json::json!({
                            "message": "Potential secrets detected in unpushed commits. Scan results saved to file.",
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
            eprintln!("No secrets detected in unpushed commits!");
        } else {
            if let Some(output_dir) = output_dir {
                let latest_file = get_latest_scan_file(&output_dir);

                match latest_file {
                    Some(file) => {
                        let file_path = file.path().to_string_lossy().to_string();
                        let content_equals = file_content_equals(&file_path, &pretty_json);

                        if content_equals {
                            eprintln!("Potential secrets detected in unpushed commits, results match previous scan. File path: {}", file_path);
                        } else {
                            let file_path = save_scan_results(&output_dir, &pretty_json);

                            if let Err(e) = file_path {
                                let scan_error =
                                    ScanInputValidationError::FailedToSaveScanResults {
                                        path: file_path.unwrap(),
                                        message: e.to_string(),
                                    };

                                let error = InputValidationError::Scan(scan_error);
                                let error_output = error.format_error_output(json_format).unwrap();

                                eprintln!("{}", error_output);
                                std::process::exit(1);
                            }

                            eprintln!("Potential secrets detected in unpushed commits. Scan results saved to: {}", file_path.unwrap());
                        }
                    }
                    None => {
                        let file_path = save_scan_results(&output_dir, &pretty_json);

                        if let Err(e) = file_path {
                            let scan_error = ScanInputValidationError::FailedToSaveScanResults {
                                path: file_path.unwrap(),
                                message: e.to_string(),
                            };

                            let error = InputValidationError::Scan(scan_error);
                            let error_output = error.format_error_output(json_format).unwrap();

                            eprintln!("{}", error_output);
                            std::process::exit(1);
                        }

                        eprintln!(
                            "Potential secrets detected in unpushed commits. Scan results saved to: {}",
                            file_path.unwrap()
                        );
                        //
                    }
                }
            } else {
                eprintln!("Potential secrets detected in unpushed commits, please review the findings before pushing to remote.");

                if let Some(skipped_commits) = &response.skipped_commits {
                    eprintln!("Skipped commits: {}", skipped_commits.join(", "));
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

pub fn get_unpushed_commit_hunks(
    context_lines: usize,
    ignore_line_comment: &str,
    exclude_patterns: &[String],
    config_file_path: Option<&str>,
) -> Result<Vec<CommitChanges>, ScanInputValidationError> {
    let repo = Repository::open(".").map_err(|e| {
        if e.code() == git2::ErrorCode::NotFound {
            ScanInputValidationError::GitRepositoryNotFound
        } else {
            ScanInputValidationError::GitRepositoryAccess {
                message: e.message().to_string(),
            }
        }
    })?;

    // Get the current branch
    let head = repo
        .head()
        .map_err(|e| ScanInputValidationError::GitHeadAccess {
            message: e.message().to_string(),
        })?;
    let local_branch_name =
        head.shorthand()
            .ok_or_else(|| ScanInputValidationError::GitBranchAccess {
                message: "Failed to get branch name".to_string(),
            })?;

    // Get the remote tracking branch
    let remote_branch = repo.find_branch(
        &format!("origin/{}", local_branch_name),
        git2::BranchType::Remote,
    );

    let local_commit =
        head.peel_to_commit()
            .map_err(|e| ScanInputValidationError::GitCommitAccess {
                message: e.message().to_string(),
            })?;

    let remote_commit = match remote_branch {
        Ok(branch) => Some(branch.get().peel_to_commit().map_err(|e| {
            ScanInputValidationError::GitCommitAccess {
                message: e.message().to_string(),
            }
        })?),
        Err(_) => None,
    };

    // If remote commit exists and is the same as local, there's nothing to push
    if let Some(rc) = &remote_commit {
        if rc.id() == local_commit.id() {
            return Ok(Vec::new());
        }
    }

    // Determine the stopping point for walking commits
    let stop_commit_id = if let Some(rc) = &remote_commit {
        // Find merge base to determine what's actually new
        if let Ok(merge_base) = repo.merge_base(local_commit.id(), rc.id()) {
            // Debug: print the commit IDs to understand what's happening
            // eprintln!("Local commit: {}", local_commit.id());
            // eprintln!("Remote commit: {}", rc.id());
            // eprintln!("Merge base: {}", merge_base);

            // If merge base equals local commit, we're behind remote
            // In this case, there are no new changes to scan
            if merge_base == local_commit.id() {
                // eprintln!("Local is behind remote - no new changes to scan");
                return Ok(Vec::new());
            }

            Some(merge_base)
        } else {
            // No common history, scan all local commits
            // eprintln!("No common history found between local and remote");
            None
        }
    } else {
        // No remote branch, scan all local commits
        // eprintln!("No remote branch found");
        None
    };

    let mut all_commit_changes = Vec::new();
    let mut current = local_commit;

    // Check all commits that would be pushed (new commits since merge base)
    loop {
        if let Some(parent) = current.parent(0).ok() {
            // Stop at merge base or remote commit
            let should_stop = stop_commit_id.map_or(false, |stop_id| stop_id == parent.id());

            // Get changes in this commit
            let parent_tree =
                parent
                    .tree()
                    .map_err(|e| ScanInputValidationError::GitTreeAccess {
                        message: e.message().to_string(),
                    })?;
            let current_tree =
                current
                    .tree()
                    .map_err(|e| ScanInputValidationError::GitTreeAccess {
                        message: e.message().to_string(),
                    })?;

            let mut diff_opts = git2::DiffOptions::new();
            diff_opts.context_lines(context_lines as u32);
            diff_opts.show_binary(false);

            let diff = repo
                .diff_tree_to_tree(
                    Some(&parent_tree),
                    Some(&current_tree),
                    Some(&mut diff_opts),
                )
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

                        let mut state = state_ref.borrow_mut();
                        let should_exclude = {
                            if let Some(&is_excluded) = state.excluded_files.get(&file_path) {
                                is_excluded
                            } else {
                                let is_excluded =
                                    should_exclude_file(&file_path, &exclude_patterns);

                                match is_excluded {
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

                        if let Some(config_file_path) = config_file_path {
                            if file_path.as_str() == config_file_path {
                                return true;
                            }
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
                                context_start_line: 1,
                                context_end_line: 2,
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
                            if file_path.as_str() == config_file_path {
                                return true;
                            }
                        }

                        let state = state_hunk.borrow();
                        // Use cached exclusion result
                        if state
                            .excluded_files
                            .get(&file_path)
                            .copied()
                            .unwrap_or(false)
                        {
                            return true;
                        }

                        // Skip hunk creation for new files as we create a single hunk in file_callback
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
                    move |delta: git2::DiffDelta,
                          _hunk: Option<git2::DiffHunk>,
                          line: git2::DiffLine| {
                        if let Some(new_file) = delta.new_file().path() {
                            let file_path = new_file.to_string_lossy().to_string();

                            let path = PathBuf::from(&file_path);
                            let extension =
                                path.extension().and_then(|ext| ext.to_str()).unwrap_or("");

                            let is_binary = is_binary_file(extension);

                            if is_binary {
                                return true;
                            }

                            let state = state_line.borrow();
                            if state
                                .excluded_files
                                .get(&file_path)
                                .copied()
                                .unwrap_or(false)
                            {
                                return true;
                            }

                            let is_new_file =
                                state.new_files.get(&file_path).copied().unwrap_or(false);
                            drop(state);

                            let mut state = state_line.borrow_mut();
                            if let Some(hunks) = state.files_with_hunks.get_mut(&file_path) {
                                let line_number =
                                    line.new_lineno().unwrap_or(line.old_lineno().unwrap_or(0))
                                        as usize;

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
                                    if let Some(hunks) = state.files_with_hunks.get_mut(&file_path)
                                    {
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
            let commit_changes: Vec<FileHunks> = FileHunks::merge_overlapping_hunks(
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

                // Only keep files that have remaining hunks after filtering
                if file.hunks.is_empty() {
                    None
                } else {
                    Some(FileHunks {
                        file_path: file.file_path,
                        hunks: file.hunks,
                    })
                }
            })
            .collect();

            let mut sorted_files = commit_changes.clone();
            sorted_files.sort_by_key(|file| file.file_path.clone());

            // Add commit metadata
            if !sorted_files.is_empty() {
                let change = CommitChanges {
                    commit_id: current.id().to_string(),
                    files: sorted_files,
                };

                all_commit_changes.push(change);
            }

            if should_stop {
                break;
            }

            current = parent;
        } else {
            // We've reached the root commit
            break;
        }
    }

    // go from the oldest commit first
    all_commit_changes.reverse();

    Ok(all_commit_changes)
}
