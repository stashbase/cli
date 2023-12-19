use std::collections::HashSet;

pub fn find_duplicates(strings: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut duplicates = HashSet::new();

    for string in strings.iter() {
        if !seen.insert(string) {
            // If the string is already in the `seen` set, it's a duplicate.
            duplicates.insert(string.clone());
        }
    }

    duplicates.into_iter().collect()
}
