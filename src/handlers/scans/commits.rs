use colored_json::to_colored_json_auto;
use git2::Repository;
use sha2::Digest;
use spinoff::{spinners, Color, Spinner, Streams};
use std::{
    cell::RefCell,
    collections::HashMap,
    io::IsTerminal,
    path::{Path, PathBuf},
    rc::Rc,
};

use crate::{
    api,
    models::{
        api_client::{OutputError, RequestApiOptionResponse},
        scans::{
            ChangeRangeWithHash, CommitChanges, CommitScanResponse, DiffHunk, FileHunks,
            PushCommitHunksPayload, ScanConfig,
        },
    },
    utils::scans::{
        filter_sha256_hashes, get_comment_prefix, is_binary_file, save_scan_results,
        should_exclude_file, should_skip_line, should_write_new_results,
    },
};

static CONTEXT_LINES: usize = 10;
static IGNORE_COMMENT: &str = "@stashbase-ignore";

pub struct HandleScanUnpushedCommitHunksArgs {
    pub api_key: String,
    pub json_format: bool,
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
        output_dir,
        config_file_path,
        ignore_value_hashes: _,
    } = args;

    let config = match &config_file_path {
        Some(path) => ScanConfig::load_from_file(path).unwrap_or_default(),
        None => ScanConfig::default(),
    };

    let enabled = config.enabled;

    if let Some(enabled) = enabled {
        if !enabled {
            if json_format {
                let message = serde_json::json!({
                    "message": "Scans are disabled in the config file."
                });
                eprintln!("{}", to_colored_json_auto(&message).unwrap());
            } else {
                eprintln!("Scans are disabled in the config file.");
            }

            std::process::exit(0);
        }
    }

    let exclude = config.exclude.unwrap_or(vec![]);

    let unpushed_commit_hunks = get_unpushed_commit_hunks(
        CONTEXT_LINES,
        &IGNORE_COMMENT,
        &exclude,
        config_file_path.as_deref(),
    )?;

    if unpushed_commit_hunks.is_empty() {
        if json_format {
            let message = serde_json::json!({
                "message": "No unpushed commits to scan."
            });
            eprintln!("{}", to_colored_json_auto(&message).unwrap());
        } else {
            eprintln!("No unpushed commits to scan.");
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

    let data = PushCommitHunksPayload {
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
                let response = serde_json::from_str::<CommitScanResponse>(&text);

                match response {
                    Ok(data) => {
                        let output_dir = match output_dir {
                            Some(dir) => Some(dir),
                            None => config.output_dir,
                        };

                        spinner.stop_and_persist("", "");

                        let is_empty = data.results.is_empty();
                        output_scan_results(data, json_format, output_dir);

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
                eprintln!("Something went wrong.");
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

fn output_scan_results(results: CommitScanResponse, json_format: bool, output_dir: Option<String>) {
    let is_empty = results.results.is_empty();

    let json_value = serde_json::to_value(&results).unwrap();
    let pretty_json = serde_json::to_string_pretty(&json_value).unwrap();

    if json_format {
        if let Some(output_dir) = output_dir {
            if is_empty {
                let message = serde_json::json!({
                    "message": "No secrets detected in unpushed commits!"
                });
                eprintln!("{}", to_colored_json_auto(&message).unwrap());
            } else {
                if should_write_new_results(&output_dir, &pretty_json) {
                    let file_path = save_scan_results(&output_dir, &pretty_json);
                    let message = serde_json::json!({
                        "message": format!("Scan results saved to: {}", file_path),
                        "file_path": file_path
                    });
                    eprintln!("{}", to_colored_json_auto(&message).unwrap());
                } else {
                    let message = serde_json::json!({
                        "message": "Results match previous scan.",
                        "file_path": output_dir
                    });
                    eprintln!("{}", to_colored_json_auto(&message).unwrap());
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
                if should_write_new_results(&output_dir, &pretty_json) {
                    let file_path = save_scan_results(&output_dir, &pretty_json);
                    eprintln!(
                        "Potential secrets detected in unpushed commits. Scan results saved to: {}",
                        file_path
                    );
                } else {
                    eprintln!(
                        "Potential secrets detected in unpushed commits. Results match previous scan."
                    );
                }
            } else {
                eprintln!("Potential secrets detected in unpushed commits, please review the findings before pushing to remote.");

                if let Some(skipped_commits) = &results.skipped_commits {
                    eprintln!("Skipped commits: {}", skipped_commits.join(", "));
                }
                eprintln!();

                if std::io::stdout().is_terminal() {
                    let result_string = results
                        .results
                        .iter()
                        .map(|result| result.get_colored_string())
                        .collect::<Vec<_>>()
                        .join("\n");

                    println!("{}", result_string);
                } else {
                    let result_string = results
                        .results
                        .iter()
                        .map(|result| format!("{}", result))
                        .collect::<Vec<_>>()
                        .join("\n\n");

                    println!("{}", result_string);
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
) -> Result<Vec<CommitChanges>, anyhow::Error> {
    let repo = Repository::open(".")?;

    // Get the current branch
    let head = repo.head()?;
    let local_branch_name = head
        .shorthand()
        .ok_or_else(|| anyhow::anyhow!("Failed to get branch name"))?;

    // Get the remote tracking branch
    let remote_branch = repo.find_branch(
        &format!("origin/{}", local_branch_name),
        git2::BranchType::Remote,
    );

    let local_commit = head.peel_to_commit()?;

    let remote_commit = match remote_branch {
        Ok(branch) => Some(branch.get().peel_to_commit()?),
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
            let parent_tree = parent.tree()?;
            let current_tree = current.tree()?;

            let mut diff_opts = git2::DiffOptions::new();
            diff_opts.context_lines(context_lines as u32);
            diff_opts.show_binary(false);

            let diff = repo.diff_tree_to_tree(
                Some(&parent_tree),
                Some(&current_tree),
                Some(&mut diff_opts),
            )?;

            let files_with_hunks = Rc::new(RefCell::new(HashMap::<String, Vec<DiffHunk>>::new()));
            // Track current change per file
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
                            let line_number = line.new_lineno().unwrap_or(0) as usize;
                            let content = String::from_utf8_lossy(line.content()).to_string();

                            let content_hash: [u8; 32] =
                                sha2::Sha256::digest(content.as_bytes()).into();

                            // Skip "No newline at end of file" messages
                            if content.trim() == "\\ No newline at end of file" {
                                return true;
                            }

                            let path = PathBuf::from(&file_path);
                            let extension =
                                path.extension().and_then(|ext| ext.to_str()).unwrap_or("");

                            if let Some(hunks) =
                                files_with_hunks_line.borrow_mut().get_mut(&file_path)
                            {
                                if let Some(last_hunk) = hunks.last_mut() {
                                    if (context_lines > 0 && line.origin() != '-')
                                        || line.origin() == '+'
                                    {
                                        last_hunk.full_content.push_str(&content);
                                    }

                                    if is_new_file {
                                        last_hunk.context_end_line = line_number;
                                    }

                                    // Check for removed ignore comments
                                    if line.origin() == '-' {
                                        if let Some(comment_prefix) = get_comment_prefix(extension)
                                        {
                                            let line_without_comment_prefix = content
                                                .trim()
                                                .trim_start_matches(comment_prefix)
                                                .trim();

                                            if line_without_comment_prefix
                                                .starts_with(&ignore_line_comment_clone)
                                            {
                                                // This is a removed ignore comment - treat it as a change
                                                let mut current_changes =
                                                    current_changes_clone.borrow_mut();
                                                let actual_line =
                                                    line.old_lineno().unwrap_or(0) as usize;

                                                match current_changes
                                                    .entry(file_path.clone())
                                                    .or_insert(None)
                                                {
                                                    Some(ref mut change) => {
                                                        // For removed lines, we need to ensure proper line number tracking
                                                        if actual_line >= change.start_line
                                                            && actual_line <= change.end_line + 3
                                                        {
                                                            change.end_line = std::cmp::max(
                                                                change.end_line,
                                                                actual_line,
                                                            );
                                                            change.content_hash = content_hash;
                                                        } else {
                                                            // Check if this content already exists in the hunk's changes
                                                            let content_exists = last_hunk
                                                                .changes
                                                                .iter()
                                                                .any(|change| {
                                                                    change.content_hash
                                                                        == content_hash
                                                                });

                                                            if !content_exists {
                                                                let change_clone = change.clone();
                                                                last_hunk
                                                                    .changes
                                                                    .push(change_clone);

                                                                let change_with_hash =
                                                                    ChangeRangeWithHash {
                                                                        start_line: actual_line,
                                                                        end_line: actual_line,
                                                                        content_hash: content_hash,
                                                                    };

                                                                *current_changes
                                                                    .get_mut(&file_path)
                                                                    .unwrap() =
                                                                    Some(change_with_hash);
                                                            }
                                                        }
                                                    }
                                                    None => {
                                                        // Check if this content already exists in the hunk's changes
                                                        let content_exists = last_hunk
                                                            .changes
                                                            .iter()
                                                            .any(|change| {
                                                                change.content_hash == content_hash
                                                            });

                                                        if !content_exists {
                                                            let change_with_hash =
                                                                ChangeRangeWithHash {
                                                                    start_line: actual_line,
                                                                    end_line: actual_line,
                                                                    content_hash: content_hash,
                                                                };

                                                            *current_changes
                                                                .get_mut(&file_path)
                                                                .unwrap() = Some(change_with_hash);
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }

                                    if line.origin() == '+' {
                                        let should_skip = if let Some(comment_prefix) =
                                            get_comment_prefix(extension)
                                        {
                                            let prev =
                                                prev_line_clone.borrow().clone().trim().to_string();

                                            let should_skip = should_skip_line(
                                                &prev,
                                                comment_prefix,
                                                &ignore_line_comment_clone,
                                            );

                                            let line_without_comment_prefix = content
                                                .trim()
                                                .trim_start_matches(comment_prefix)
                                                .trim();

                                            should_skip
                                                || line_without_comment_prefix
                                                    .starts_with(&ignore_line_comment_clone)
                                        } else {
                                            false
                                        };

                                        let is_blank_line = content.trim().is_empty();

                                        if !should_skip {
                                            let mut current_changes =
                                                current_changes_clone.borrow_mut();

                                            // For new files, create a single change range
                                            if is_new_file {
                                                match current_changes
                                                    .entry(file_path.clone())
                                                    .or_insert(None)
                                                {
                                                    Some(ref mut change) => {
                                                        // Continue existing change
                                                        change.end_line = line_number;
                                                        change.content_hash = content_hash;
                                                    }
                                                    None => {
                                                        // Skip leading blank lines
                                                        if !is_blank_line {
                                                            // Check if this content already exists in the hunk's changes
                                                            let content_exists = last_hunk
                                                                .changes
                                                                .iter()
                                                                .any(|change| {
                                                                    change.content_hash
                                                                        == content_hash
                                                                });

                                                            if !content_exists {
                                                                let change_with_hash =
                                                                    ChangeRangeWithHash {
                                                                        start_line: line_number,
                                                                        end_line: line_number,
                                                                        content_hash: content_hash,
                                                                    };

                                                                *current_changes
                                                                    .get_mut(&file_path)
                                                                    .unwrap() =
                                                                    Some(change_with_hash);
                                                            }
                                                        }
                                                    }
                                                }
                                            } else {
                                                // For modified files, handle normally
                                                match current_changes
                                                    .entry(file_path.clone())
                                                    .or_insert(None)
                                                {
                                                    Some(ref mut change) => {
                                                        // Continue existing change if it's within reasonable range
                                                        if line_number <= change.end_line + 3 {
                                                            // Always include the line if we're in the middle of a change
                                                            // Ensure end_line is at least the current line_number
                                                            change.end_line = std::cmp::max(
                                                                change.end_line,
                                                                line_number,
                                                            );
                                                            change.content_hash = content_hash;
                                                        } else {
                                                            // Check if this content already exists in the hunk's changes
                                                            let content_exists = last_hunk
                                                                .changes
                                                                .iter()
                                                                .any(|change| {
                                                                    change.content_hash
                                                                        == content_hash
                                                                });

                                                            if !content_exists {
                                                                // Gap too large, create new change range
                                                                let change_clone = change.clone();
                                                                last_hunk
                                                                    .changes
                                                                    .push(change_clone);

                                                                let change_with_hash =
                                                                    ChangeRangeWithHash {
                                                                        start_line: line_number,
                                                                        end_line: line_number,
                                                                        content_hash: content_hash,
                                                                    };
                                                                // Don't start new change if it's a blank line
                                                                if !is_blank_line {
                                                                    *current_changes
                                                                        .get_mut(&file_path)
                                                                        .unwrap() =
                                                                        Some(change_with_hash);
                                                                }
                                                            }
                                                        }
                                                    }
                                                    None => {
                                                        // Don't start new change if it's a blank line
                                                        if !is_blank_line {
                                                            // Check if this content already exists in the hunk's changes
                                                            let content_exists = last_hunk
                                                                .changes
                                                                .iter()
                                                                .any(|change| {
                                                                    change.content_hash
                                                                        == content_hash
                                                                });

                                                            if !content_exists {
                                                                let change_with_hash =
                                                                    ChangeRangeWithHash {
                                                                        start_line: line_number,
                                                                        end_line: line_number,
                                                                        content_hash: content_hash,
                                                                    };

                                                                *current_changes
                                                                    .get_mut(&file_path)
                                                                    .unwrap() =
                                                                    Some(change_with_hash);
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    } else if let Some((_file_path, Some(change))) =
                                        current_changes_clone.borrow_mut().iter_mut().find_map(
                                            |(k, v)| {
                                                if v.is_some() {
                                                    Some((k.clone(), v.take()))
                                                } else {
                                                    None
                                                }
                                            },
                                        )
                                    {
                                        let adjusted_end_line = change.end_line;
                                        let change_range = ChangeRangeWithHash {
                                            start_line: change.start_line,
                                            end_line: adjusted_end_line,
                                            content_hash: content_hash,
                                        };

                                        last_hunk.changes.push(change_range);
                                    }
                                }
                            }

                            if line.origin() != '-' {
                                *prev_line_clone.borrow_mut() = content;
                            }
                        }
                        true
                    };

                diff.foreach(
                    &mut file_callback,
                    None,
                    Some(&mut hunk_callback),
                    Some(&mut line_callback),
                )?;

                if let Some((_file_path, Some(change))) =
                    current_changes.borrow_mut().iter_mut().find_map(|(k, v)| {
                        if v.is_some() {
                            Some((k.clone(), v.take()))
                        } else {
                            None
                        }
                    })
                {
                    if let Some(hunks) = files_with_hunks.borrow_mut().values_mut().last() {
                        if let Some(last_hunk) = hunks.last_mut() {
                            last_hunk.changes.push(change);
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

            // Add commit metadata
            if !commit_changes.is_empty() {
                let change = CommitChanges {
                    commit_id: current.id().to_string(),
                    files: commit_changes,
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

    Ok(all_commit_changes)
}
