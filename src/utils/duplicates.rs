use std::collections::HashMap;

pub fn find_duplicates(array: &Vec<String>) -> Vec<String> {
    let mut key_count = HashMap::new();

    // Count occurrences of each key
    for item in array {
        *key_count.entry(item).or_insert(0) += 1;
    }

    // Collect keys with more than one occurrence
    key_count
        .into_iter()
        .filter_map(|(key, count)| if count > 1 { Some(key.clone()) } else { None })
        .collect()
}
