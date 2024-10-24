use std::collections::HashMap;

pub fn find_duplicates(array: &Vec<String>) -> Vec<String> {
    let mut item_count = HashMap::new();

    // Count occurrences of each key
    for item in array {
        *item_count.entry(item).or_insert(0) += 1;
    }

    // Collect keys with more than one occurrence
    item_count
        .into_iter()
        .filter_map(|(key, count)| if count > 1 { Some(key.clone()) } else { None })
        .collect()
}
