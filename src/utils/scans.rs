use crate::models::scans::DiffHunk;

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

