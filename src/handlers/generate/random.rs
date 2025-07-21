use nanoid::nanoid;

use crate::{models::generate::Encoding, utils::output::get_formatted_json_string};

pub fn handle_generate_random_string(
    encooding: Encoding,
    json_format: bool,
    length: usize,
    uppercase: bool,
) {
    let alphabet = encooding.get_alphabet();
    let result = nanoid!(length, &alphabet.chars().collect::<Vec<char>>());

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
