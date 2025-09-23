use rand::{rng, Rng};

use crate::{models::generate::Encoding, utils::output::get_formatted_json_string};

pub fn handle_generate_random_string(
    encoding: Encoding,
    json_format: bool,
    length: usize,
    uppercase: bool,
) {
    let alphabet = encoding.get_alphabet();
    let alphabet_chars: Vec<char> = alphabet.chars().collect();
    let alphabet_len = alphabet_chars.len();

    let mut rng = rng();
    let mut result = String::with_capacity(length);

    for _ in 0..length {
        let idx = rng.random_range(0..alphabet_len);

        result.push(alphabet_chars[idx]);
    }

    let final_result = if uppercase {
        result.to_uppercase()
    } else {
        result
    };

    if json_format {
        let json_output = serde_json::json!({
            "value": final_result
        });
        let json_str = get_formatted_json_string(&json_output, true).unwrap();
        println!("{}", json_str);
    } else {
        println!("{}", final_result);
    }
}
