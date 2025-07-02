use sha2::{Sha256, Digest};
use std::{path::Path, fs};
use ignore::gitignore::GitignoreBuilder;
use crate::models::scans::DiffHunk;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn should_merge_hunks(hunk1: &DiffHunk, hunk2: &DiffHunk, max_gap: usize) -> bool {
    // Only merge if they're close enough
    if (hunk2.context_start_line as i64 - hunk1.context_end_line as i64).abs() > max_gap as i64 {
        return false;
    }

    // Check for context overlap
    hunk1.context_end_line >= hunk2.context_start_line
        || (hunk2.context_start_line - hunk1.context_end_line) <= max_gap
}

pub fn get_comment_prefix(extension: &str) -> Option<&'static str> {
    match extension {
        "rs" | "js" | "ts" | "tsx" | "jsx" | "vue" | "java" | "cpp" | "c" | "cs" | "go "
        | "php" | "kt" | "scala" | "dart" | "svelte" | "m" | "mm" => Some("//"),
        "py" | "sh" | "toml" | "yaml" | "yml" | "ini" | "r" | "swift" | "rb" | "dockerfile"
        | "makefile" | "ex" | "es" | "exs" | "pl" => Some("#"),
        "sql" | "hs" | "lua" => Some("--"),
        _ => None,
    }
}

pub fn is_comment_line(line: &str, comment_prefix: &str) -> bool {
    line.trim_start().starts_with(comment_prefix)
}

pub fn should_skip_line(line: &str, comment_prefix: &str, skip_comment: &str) -> bool {
    if is_comment_line(line, comment_prefix) {
        let trimmed = line.trim_start();

        if trimmed.starts_with(comment_prefix) {
            let without_prefix = trimmed.trim_start_matches(comment_prefix).trim_start();
            // println!("without_prefix: {}", without_prefix);

            if without_prefix == String::from(skip_comment) {
                return true;
            }
        }
    }

    false
}

pub fn is_binary_file(extension: &str) -> bool {
    match extension {
        // Compiled files
        "exe" | "dll" | "so" | "dylib" | "class" | "o" | "obj" | "pyc" | "pyo" |
        
        // Compressed archives
        "zip" | "tar" | "gz" | "bz2" | "7z" | "rar" | "xz" |
        
        // Media files
        "jpg" | "jpeg" | "png" | "gif" | "bmp" | "ico" | "webp" | "tiff" | // Images
        "mp3" | "wav" | "ogg" | "flac" | "m4a" | "wma" | // Audio
        "mp4" | "avi" | "mkv" | "mov" | "wmv" | "flv" | "webm" | // Video
        
        // Document formats
        "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" |
        
        // Database files
        "db" | "sqlite" | "mdb" | "frm" | "myd" | "myi" |
        
        // Other binary formats
        "bin" | "iso" | "img" | "dat" => true,
        
        // Everything else is considered text
        _ => false,
    }
}

pub fn should_exclude_file(file_path: &str, exclude_patterns: &[String]) -> bool {
    let mut builder = GitignoreBuilder::new("/"); // Root directory
    for pattern in exclude_patterns {
        builder.add_line(None, pattern).unwrap();
    }
    let gitignore = builder.build().unwrap();
    gitignore.matched(Path::new(file_path), false).is_ignore()
}


pub fn calculate_hash(content: &str) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    hasher.finalize().to_vec()
}

pub fn get_latest_scan_file(output_dir: &str) -> Option<std::fs::DirEntry> {
    let scan_dir = Path::new(output_dir);
    fs::read_dir(scan_dir)
        .ok()?
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().extension().and_then(|ext| ext.to_str()) == Some("json"))
        .max_by_key(|entry| entry.path())
}

pub fn should_write_new_results(output_dir: &str, new_content: &str) -> bool {
    if let Some(latest_file) = get_latest_scan_file(output_dir) {
        // Read and hash the content of the latest file
        if let Ok(content) = fs::read_to_string(latest_file.path()) {
            let existing_hash = calculate_hash(&content);
            let new_hash = calculate_hash(new_content);
            return new_hash != existing_hash;
        }
    }
    true // No existing file or couldn't read it, should write
}

pub fn save_scan_results(output_dir: &str, json_content: &str) -> String {
    // Create scan_results directory if it doesn't exist
    fs::create_dir_all(output_dir).unwrap();

    // Get current timestamp
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    // Create file path
    let file_path = format!("{}/{}.json", output_dir, timestamp);

    // Write to file
    fs::write(&file_path, json_content).unwrap();

    file_path
}

pub fn filter_sha256_hashes(hashes: Vec<String>) -> Vec<String> {
    hashes
        .into_iter()
        .filter(|h| h.len() == 64 && h.chars().all(|c| c.is_ascii_hexdigit()))
        .collect::<Vec<_>>()
}