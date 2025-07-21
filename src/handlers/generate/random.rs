use nanoid::nanoid;

pub enum GenerateRandomValueVariant {
    Alphanumeric,
    Hex,
    Base64,
    Base64Url,
}

pub fn handle_generate_random_value(
    variant: GenerateRandomValueVariant,
    json_format: bool,
    length: usize,
    uppercase: bool,
) {
    let alphabet = match variant {
        GenerateRandomValueVariant::Alphanumeric => {
            // 0-9, a-z, A-Z (62 characters)
            "0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ"
        }
        GenerateRandomValueVariant::Hex => {
            // 0-9, a-f (16 characters)
            "0123456789abcdef"
        }
        GenerateRandomValueVariant::Base64 => {
            // A-Z, a-z, 0-9, +, / (64 characters)
            "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"
        }
        GenerateRandomValueVariant::Base64Url => {
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
        // Use 'Id: value' format for silent mode as per memory
        println!("Random: {}", final_result);
    }
}
