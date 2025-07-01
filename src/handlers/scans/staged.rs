use crate::{
    models::scans::{DiffHunk, FileHunks, LineRange},
    utils::scans::{get_comment_prefix, is_binary_file, should_exclude_file, should_skip_line},
};
use git2::Repository;
use std::{
    cell::RefCell,
    collections::HashMap,
    path::{Path, PathBuf},
    rc::Rc,
};

pub fn get_staged_file_hunks(
    context_lines: usize,
    config_file_path: &str,
    ignore_line_comment: &str,
    exclude_patterns: &[String],
) -> Result<Vec<FileHunks>, anyhow::Error> {
    let repo = Repository::open(".")?;
    let repo_for_head = Repository::open(".")?;
    let index = repo.index()?;

    let head_tree = match repo_for_head.head() {
        Ok(head) => head.peel_to_tree()?,
        Err(_) => {
            let empty_tree = repo_for_head.treebuilder(None)?;
            let oid = empty_tree.write()?;
            repo_for_head.find_tree(oid)?
        }
    };

    let mut diff_opts = git2::DiffOptions::new();
    diff_opts.context_lines(context_lines as u32);
    diff_opts.show_binary(false);

    let diff = repo.diff_tree_to_index(Some(&head_tree), Some(&index), Some(&mut diff_opts))?;

    let files_with_hunks = Rc::new(RefCell::new(HashMap::<String, Vec<DiffHunk>>::new()));
    // Track current change per file
    let current_changes = Rc::new(RefCell::new(HashMap::<String, Option<LineRange>>::new()));
    let prev_line = Rc::new(RefCell::new(String::new()));

    // Create a cache for excluded files
    let excluded_files = Rc::new(RefCell::new(HashMap::<String, bool>::new()));
    // Create a cache for new files
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

                let extension = Path::new(&file_path)
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .unwrap_or("");

                let is_binary = is_binary_file(extension);

                if is_binary {
                    return true;
                }

                if file_path == config_file_path {
                    return true;
                }

                // Check if we've already determined if this file is excluded
                let should_exclude = {
                    let mut cache = excluded_files_ref.borrow_mut();
                    if let Some(&is_excluded) = cache.get(&file_path) {
                        is_excluded
                    } else {
                        let is_excluded = should_exclude_file(&file_path, &exclude_patterns);
                        cache.insert(file_path.clone(), is_excluded);
                        is_excluded
                    }
                };

                if should_exclude {
                    return true; // Skip this file and continue to next
                }

                // Check and store if this is a new file
                let is_new_file = delta.status() == git2::Delta::Added;
                new_files_ref
                    .borrow_mut()
                    .insert(file_path.clone(), is_new_file);

                if is_new_file {
                    // For new files, create a single hunk that covers the entire file
                    let mut files = files_with_hunks_ref.borrow_mut();
                    let hunks = files.entry(file_path).or_insert_with(Vec::new);

                    // Create a single hunk for the entire file
                    let hunk = DiffHunk {
                        full_content: String::new(), // Will be populated in line callback
                        changes: Vec::new(),         // Will be populated in line callback
                        context_end_line: 2,         // Will be updated in line callback
                        context_start_line: 1,
                    };

                    hunks.push(hunk);
                } else {
                    // For modified files, process normally
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

                if file_path == config_file_path {
                    return true;
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

        let mut line_callback = move |delta: git2::DiffDelta,
                                      _hunk: Option<git2::DiffHunk>,
                                      line: git2::DiffLine| {
            if let Some(new_file) = delta.new_file().path() {
                let file_path = new_file.to_string_lossy().to_string();

                // Use cached exclusion result
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

                // Skip "No newline at end of file" messages
                if content.trim() == "\\ No newline at end of file" {
                    return true;
                }

                // Get file extension to determine comment prefix
                let path = PathBuf::from(&file_path);
                let extension = path.extension().and_then(|ext| ext.to_str()).unwrap_or("");

                if let Some(hunks) = files_with_hunks_line.borrow_mut().get_mut(&file_path) {
                    if let Some(last_hunk) = hunks.last_mut() {
                        // Add to full_content if it's a context line (not '-') or it's an addition
                        if (context_lines > 0 && line.origin() != '-') || line.origin() == '+' {
                            last_hunk.full_content.push_str(&content);
                        }

                        // For new files, update the end line number
                        if is_new_file {
                            last_hunk.context_end_line = line_number;
                        }

                        // Check for removed ignore comments
                        if line.origin() == '-' {
                            if let Some(comment_prefix) = get_comment_prefix(extension) {
                                let line_without_comment_prefix =
                                    content.trim().trim_start_matches(comment_prefix).trim();

                                if line_without_comment_prefix
                                    .starts_with(&ignore_line_comment_clone)
                                {
                                    // This is a removed ignore comment - treat it as a change
                                    let mut current_changes = current_changes_clone.borrow_mut();
                                    let actual_line = line.old_lineno().unwrap_or(0) as usize;

                                    match current_changes.entry(file_path.clone()).or_insert(None) {
                                        Some(ref mut change) => {
                                            // For removed lines, we need to ensure proper line number tracking
                                            if actual_line >= change.start_line
                                                && actual_line <= change.end_line + 3
                                            {
                                                change.end_line =
                                                    std::cmp::max(change.end_line, actual_line);
                                                change.content.push_str(&content);
                                            } else {
                                                // Check if this content already exists in the hunk's changes
                                                let content_exists = last_hunk
                                                    .changes
                                                    .iter()
                                                    .any(|change| change.content == content);

                                                if !content_exists {
                                                    let change_clone = change.clone();
                                                    last_hunk.changes.push(change_clone);
                                                    *current_changes.get_mut(&file_path).unwrap() =
                                                        Some(LineRange {
                                                            start_line: actual_line,
                                                            end_line: actual_line,
                                                            content: content.clone(),
                                                        });
                                                }
                                            }
                                        }
                                        None => {
                                            // Check if this content already exists in the hunk's changes
                                            let content_exists = last_hunk
                                                .changes
                                                .iter()
                                                .any(|change| change.content == content);

                                            if !content_exists {
                                                *current_changes.get_mut(&file_path).unwrap() =
                                                    Some(LineRange {
                                                        start_line: actual_line,
                                                        end_line: actual_line,
                                                        content: content.clone(),
                                                    });
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        // Handle changes
                        if line.origin() == '+' {
                            // Check if previous line has a skip comment
                            let should_skip =
                                if let Some(comment_prefix) = get_comment_prefix(extension) {
                                    let prev = prev_line_clone.borrow().clone().trim().to_string();

                                    let should_skip = should_skip_line(
                                        &prev,
                                        comment_prefix,
                                        &ignore_line_comment_clone,
                                    );

                                    let line_without_comment_prefix =
                                        content.trim().trim_start_matches(comment_prefix).trim();

                                    should_skip
                                        || line_without_comment_prefix
                                            .starts_with(&ignore_line_comment_clone)
                                } else {
                                    false
                                };

                            let is_blank_line = content.trim().is_empty();

                            if !should_skip {
                                let mut current_changes = current_changes_clone.borrow_mut();

                                // For new files, create a single change range
                                if is_new_file {
                                    match current_changes.entry(file_path.clone()).or_insert(None) {
                                        Some(ref mut change) => {
                                            // Continue existing change
                                            change.end_line =
                                                std::cmp::max(change.end_line, line_number);
                                            change.content.push_str(&content);
                                        }
                                        None => {
                                            // Skip leading blank lines
                                            if !is_blank_line {
                                                // Check if this content already exists in the hunk's changes
                                                let content_exists = last_hunk
                                                    .changes
                                                    .iter()
                                                    .any(|change| change.content == content);

                                                if !content_exists {
                                                    *current_changes.get_mut(&file_path).unwrap() =
                                                        Some(LineRange {
                                                            start_line: line_number,
                                                            end_line: line_number,
                                                            content: content.clone(),
                                                        });
                                                }
                                            }
                                        }
                                    }
                                } else {
                                    // For modified files, handle normally
                                    match current_changes.entry(file_path.clone()).or_insert(None) {
                                        Some(ref mut change) => {
                                            // Continue existing change if it's within reasonable range
                                            if line_number <= change.end_line + 3 {
                                                // Always include the line if we're in the middle of a change
                                                change.end_line =
                                                    std::cmp::max(change.end_line, line_number);
                                                change.content.push_str(&content);
                                            } else {
                                                // Check if this content already exists in the hunk's changes
                                                let content_exists = last_hunk
                                                    .changes
                                                    .iter()
                                                    .any(|change| change.content == content);

                                                if !content_exists {
                                                    // Gap too large, create new change range
                                                    let change_clone = change.clone();
                                                    last_hunk.changes.push(change_clone);
                                                    // Don't start new change if it's a blank line
                                                    if !is_blank_line {
                                                        *current_changes
                                                            .get_mut(&file_path)
                                                            .unwrap() = Some(LineRange {
                                                            start_line: line_number,
                                                            end_line: line_number,
                                                            content: content.clone(),
                                                        });
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
                                                    .any(|change| change.content == content);

                                                if !content_exists {
                                                    *current_changes.get_mut(&file_path).unwrap() =
                                                        Some(LineRange {
                                                            start_line: line_number,
                                                            end_line: line_number,
                                                            content: content.clone(),
                                                        });
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                // Update previous line content
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

        // Don't forget to add the last change if there is one
        let current_changes = current_changes.borrow_mut();
        for (file_path, change_opt) in current_changes.iter() {
            if let Some(change) = change_opt {
                if let Some(hunks) = files_with_hunks.borrow_mut().get_mut(file_path) {
                    if let Some(last_hunk) = hunks.last_mut() {
                        last_hunk.changes.push(change.clone());
                    }
                }
            }
        }
    }

    //

    let result: Vec<FileHunks> = FileHunks::merge_overlapping_hunks(
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
    // Filter out files that have no hunks with changes
    .filter(|file| {
        // Keep only hunks that have non-empty changes
        let non_empty_hunks: Vec<DiffHunk> = file
            .hunks
            .iter()
            .filter(|hunk| !hunk.changes.is_empty())
            .cloned()
            .collect();

        // Update the file's hunks to only include non-empty ones
        !non_empty_hunks.is_empty()
    })
    .map(|mut file| {
        // Update the file's hunks to only include non-empty ones
        file.hunks = file
            .hunks
            .into_iter()
            .filter(|hunk| !hunk.changes.is_empty())
            .collect();
        file
    })
    .collect();

    Ok(result)
}
