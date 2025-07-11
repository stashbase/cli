use colored_json::to_colored_json_auto;
use git2::Repository;
use spinoff::{spinners, Color, Spinner, Streams};
use std::{cell::RefCell, collections::HashMap, io::IsTerminal, path::PathBuf, rc::Rc};

use crate::{
    api,
    models::{
        api_client::{GenericOutputError, OutputError, RequestApiOptionResponse},
        scans::{
            ChangeRangeWithHash, CommitChanges, CommitsScanResponse, DiffHunk, FileHunks,
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
        "Scanning unpushed commits...",
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

                                    eprintln!("\n{}", error_output);
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
                        eprintln!("\nSomething went wrong.");
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
                            eprintln!("Potential secrets detected in unpushed commits. Scan results saved to: {}", file_path);
                        }
                    }
                    None => {
                        let file_path = save_scan_results(&output_dir, &pretty_json);
                        eprintln!(
                            "Potential secrets detected in unpushed commits. Scan results saved to: {}",
                            file_path
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
                        .join("\n");

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

            let files_with_hunks = Rc::new(RefCell::new(HashMap::<String, Vec<DiffHunk>>::new()));
            // Track current change per hunk (file_path + hunk_index)
            let current_changes = Rc::new(RefCell::new(HashMap::<
                String,
                Option<ChangeRangeWithHash>,
            >::new()));
            let prev_line = Rc::new(RefCell::new(String::new()));

            let excluded_files = Rc::new(RefCell::new(HashMap::<String, bool>::new()));
            let new_files = Rc::new(RefCell::new(HashMap::<String, bool>::new()));

            let ignore_line_comment = ignore_line_comment.to_string();

            {
                let excluded_files_ref = Rc::clone(&excluded_files);
                let files_with_hunks_ref = Rc::clone(&files_with_hunks);
                let new_files_ref = Rc::clone(&new_files);
                // let current_changes_ref = Rc::clone(&current_changes);
                let mut file_callback = |delta: git2::DiffDelta, _progress: f32| {
                    if let Some(new_file) = delta.new_file().path() {
                        let file_path = new_file.to_string_lossy().to_string();

                        let should_exclude = {
                            let mut cache = excluded_files_ref.borrow_mut();
                            if let Some(&is_excluded) = cache.get(&file_path) {
                                is_excluded
                            } else {
                                let is_excluded =
                                    should_exclude_file(&file_path, &exclude_patterns);
                                cache.insert(file_path.clone(), is_excluded);
                                is_excluded
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
                        new_files_ref
                            .borrow_mut()
                            .insert(file_path.clone(), is_new_file);

                        if is_new_file {
                            let mut files = files_with_hunks_ref.borrow_mut();
                            let hunks = files.entry(file_path).or_insert_with(Vec::new);

                            let hunk = DiffHunk {
                                full_content: String::new(),
                                changes: Vec::new(),
                                context_start_line: 1,
                                context_end_line: 2,
                            };

                            hunks.push(hunk);
                        } else {
                            files_with_hunks_ref
                                .borrow_mut()
                                .entry(file_path)
                                .or_insert_with(Vec::new);
                        }
                    }
                    true
                };

                let files_with_hunks_hunk = Rc::clone(&files_with_hunks);
                let excluded_files_hunk = Rc::clone(&excluded_files);
                let new_files_hunk = Rc::clone(&new_files);
                // let current_changes_hunk = Rc::clone(&current_changes);
                let mut hunk_callback = move |delta: git2::DiffDelta, hunk: git2::DiffHunk| {
                    if let Some(new_file) = delta.new_file().path() {
                        let file_path = new_file.to_string_lossy().to_string();

                        if let Some(config_file_path) = config_file_path {
                            if file_path.as_str() == config_file_path {
                                return true;
                            }
                        }

                        // Use cached exclusion result
                        if excluded_files_hunk
                            .borrow()
                            .get(&file_path)
                            .copied()
                            .unwrap_or(false)
                        {
                            return true;
                        }

                        // Skip hunk creation for new files as we create a single hunk in file_callback
                        if new_files_hunk
                            .borrow()
                            .get(&file_path)
                            .copied()
                            .unwrap_or(false)
                        {
                            return true;
                        }

                        let hunk_with_context = DiffHunk {
                            full_content: String::new(),
                            changes: Vec::new(),
                            context_start_line: hunk.new_start() as usize,
                            context_end_line: (hunk.new_start() + hunk.new_lines()) as usize,
                        };

                        files_with_hunks_hunk
                            .borrow_mut()
                            .entry(file_path)
                            .or_insert_with(Vec::new)
                            .push(hunk_with_context);
                    }
                    true
                };

                let current_changes_clone = Rc::clone(&current_changes);
                let files_with_hunks_line = Rc::clone(&files_with_hunks);
                let prev_line_clone = Rc::clone(&prev_line);
                let excluded_files_line = Rc::clone(&excluded_files);
                let new_files_line = Rc::clone(&new_files);
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

                            if excluded_files_line
                                .borrow()
                                .get(&file_path)
                                .copied()
                                .unwrap_or(false)
                            {
                                return true;
                            }

                            let is_new_file = new_files_line
                                .borrow()
                                .get(&file_path)
                                .copied()
                                .unwrap_or(false);

                            if let Some(hunks) =
                                files_with_hunks_line.borrow_mut().get_mut(&file_path)
                            {
                                // Find the correct hunk based on line number instead of always using the last one
                                let line_number =
                                    line.new_lineno().unwrap_or(line.old_lineno().unwrap_or(0))
                                        as usize;

                                let target_hunk_index = if is_new_file {
                                    // For new files, always use the first (and only) hunk
                                    Some(0)
                                } else {
                                    // For modified files, find the hunk that contains this line number
                                    hunks.iter().position(|hunk| {
                                        line_number >= hunk.context_start_line
                                            && line_number <= hunk.context_end_line
                                    })
                                };

                                if let Some(hunk_index) = target_hunk_index {
                                    if let Some(target_hunk) = hunks.get_mut(hunk_index) {
                                        let mut current_changes =
                                            current_changes_clone.borrow_mut();
                                        // Create a unique key per hunk to track changes separately
                                        let hunk_key = format!("{}:{}", file_path, hunk_index);
                                        let mut current_change =
                                            current_changes.entry(hunk_key).or_insert(None);
                                        let mut prev_line_content = prev_line_clone.borrow_mut();

                                        process_diff_line(
                                            line,
                                            &file_path,
                                            is_new_file,
                                            &mut current_change,
                                            target_hunk,
                                            &mut prev_line_content,
                                            &ignore_line_comment_clone,
                                            context_lines,
                                        );
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

                let current_changes = current_changes.borrow_mut();
                for (hunk_key, change_opt) in current_changes.iter() {
                    if let Some(change) = change_opt {
                        // Parse the hunk key to get file_path and hunk_index
                        if let Some((file_path, hunk_index_str)) = hunk_key.split_once(':') {
                            if let Ok(hunk_index) = hunk_index_str.parse::<usize>() {
                                if let Some(hunks) =
                                    files_with_hunks.borrow_mut().get_mut(file_path)
                                {
                                    if let Some(target_hunk) = hunks.get_mut(hunk_index) {
                                        target_hunk.changes.push(change.clone());
                                    }
                                }
                            }
                        }
                    }
                }
            }

            let commit_changes: Vec<FileHunks> = FileHunks::merge_overlapping_hunks(
                files_with_hunks
                    .borrow()
                    .iter()
                    .map(|(file_path, hunks)| FileHunks {
                        file_path: file_path.clone(),
                        hunks: hunks.clone(),
                    })
                    .collect(),
                context_lines,
            )
            .into_iter()
            .filter(|file| !file.hunks.iter().all(|hunk| hunk.changes.is_empty()))
            .map(|mut file| {
                file.hunks = file
                    .hunks
                    .into_iter()
                    .filter(|hunk| !hunk.changes.is_empty())
                    .collect();
                FileHunks {
                    file_path: file.file_path,
                    hunks: file.hunks,
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
