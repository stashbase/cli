use nanoid::nanoid;

use crate::models::generate::GenerateRandomValueAlphabet;

pub fn handle_generate_random_value(
    alphabet: GenerateRandomValueAlphabet,
    json_format: bool,
    length: usize,
    uppercase: bool,
) {
    let alphabet = match alphabet {
        GenerateRandomValueAlphabet::Alphanumeric => {
            // 0-9, a-z, A-Z (62 characters)
            "0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ"
        }
        GenerateRandomValueAlphabet::Hexadecimal => {
            // 0-9, a-f (16 characters)
            "0123456789abcdef"
        }
        GenerateRandomValueAlphabet::Base64 => {
            // A-Z, a-z, 0-9, +, / (64 characters)
            "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"
        }
        GenerateRandomValueAlphabet::Base64Url => {
            // A-Z, a-z, 0-9, -, _ (64 characters) - URL-safe base64
            "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_"
        }
    };

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
        println!("{}", json_output);
    } else {
        println!("{}", final_result);
    }
}
