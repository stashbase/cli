use std::path::Path;

pub fn check_file_exists(path_str: &Path) -> bool {
    // let path = Path::new(&path_str);
    let file_exists = path_str.exists();

    if !path_str.is_file() {
        false
    } else {
        file_exists
    }
}
