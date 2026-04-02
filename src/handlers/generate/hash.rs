use sha2::{Digest, Sha224, Sha256, Sha384, Sha512};

use crate::{
    cmd::generate::GenerateHashAlgorithm, utils::output::get_formatted_json_string,
};

pub fn handle_generate_hash(
    value: String,
    algorithm: GenerateHashAlgorithm,
    json_format: bool,
    uppercase: bool,
) {
    let hash = generate_hash(value.as_bytes(), algorithm);
    let output = if uppercase { hash.to_uppercase() } else { hash };

    if json_format {
        let json = serde_json::json!({ "value": output });
        let json_pretty = get_formatted_json_string(&json, true).unwrap();
        println!("{}", json_pretty);
    } else {
        println!("{}", output);
    }
}

fn generate_hash(value: &[u8], algorithm: GenerateHashAlgorithm) -> String {
    match algorithm {
        GenerateHashAlgorithm::Sha224 => format!("{:x}", Sha224::digest(value)),
        GenerateHashAlgorithm::Sha256 => format!("{:x}", Sha256::digest(value)),
        GenerateHashAlgorithm::Sha384 => format!("{:x}", Sha384::digest(value)),
        GenerateHashAlgorithm::Sha512 => format!("{:x}", Sha512::digest(value)),
    }
}
