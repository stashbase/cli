use anyhow::{Context, Result};
use base64::{
    engine::general_purpose::{STANDARD_NO_PAD, URL_SAFE_NO_PAD},
    Engine as _,
};
use data_encoding::BASE32_NOPAD;
use rand::rngs::OsRng;
use rand::TryRngCore;

use crate::{models::generate::Encoding, utils::output::get_formatted_json_string};

pub fn handle_generate_random_string(
    encoding: Encoding,
    json_format: bool,
    length: usize,
    bytes: Option<u16>,
    uppercase: bool,
) -> Result<()> {
    let mut rng = OsRng;
    let result = if let Some(bytes_len) = bytes {
        let mut raw = vec![0u8; bytes_len as usize];
        rng.try_fill_bytes(&mut raw)
            .context("Failed to read secure random bytes from OS RNG")?;
        encode_random_bytes(&encoding, &raw)
    } else {
        match encoding {
            Encoding::Hex => {
                generate_power_of_two_string(&mut rng, b"0123456789abcdef", 4, length)?
            }
            Encoding::Base32 => generate_power_of_two_string(
                &mut rng,
                b"abcdefghijklmnopqrstuvwxyz234567",
                5,
                length,
            )?,
            Encoding::Base64 => generate_power_of_two_string(
                &mut rng,
                b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/",
                6,
                length,
            )?,
            Encoding::Base64Url => generate_power_of_two_string(
                &mut rng,
                b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_",
                6,
                length,
            )?,
            Encoding::Alphanumeric => generate_alphanumeric_string(&mut rng, length)?,
        }
    };

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

    Ok(())
}

fn encode_random_bytes(encoding: &Encoding, bytes: &[u8]) -> String {
    match encoding {
        Encoding::Hex => hex::encode(bytes),
        Encoding::Base32 => BASE32_NOPAD.encode(bytes).to_lowercase(),
        Encoding::Base64 => STANDARD_NO_PAD.encode(bytes),
        Encoding::Base64Url => URL_SAFE_NO_PAD.encode(bytes),
        Encoding::Alphanumeric => encode_base62(bytes),
    }
}

fn generate_power_of_two_string(
    rng: &mut OsRng,
    alphabet: &[u8],
    bits_per_symbol: u8,
    length: usize,
) -> Result<String> {
    let mut output = String::with_capacity(length);
    let mut bit_buffer: u32 = 0;
    let mut bits_in_buffer: u8 = 0;
    let mask: u32 = (1u32 << bits_per_symbol) - 1;

    while output.len() < length {
        if bits_in_buffer < bits_per_symbol {
            let byte = next_random_byte(rng)?;
            bit_buffer = (bit_buffer << 8) | (byte as u32);
            bits_in_buffer += 8;
            continue;
        }

        let shift = bits_in_buffer - bits_per_symbol;
        let idx = ((bit_buffer >> shift) & mask) as usize;
        output.push(alphabet[idx] as char);

        bits_in_buffer = shift;
        bit_buffer &= (1u32 << bits_in_buffer) - 1;
    }

    Ok(output)
}

fn generate_alphanumeric_string(rng: &mut OsRng, length: usize) -> Result<String> {
    const ALPHANUMERIC: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";
    const ALPHANUMERIC_LEN: u8 = 62;
    const REJECTION_BOUND: u8 = 248;

    let mut output = String::with_capacity(length);

    while output.len() < length {
        let value = next_random_byte(rng)?;
        if value < REJECTION_BOUND {
            let idx = (value % ALPHANUMERIC_LEN) as usize;
            output.push(ALPHANUMERIC[idx] as char);
        }
    }

    Ok(output)
}

fn next_random_byte(rng: &mut OsRng) -> Result<u8> {
    let mut buf = [0u8; 1];
    rng.try_fill_bytes(&mut buf)
        .context("Failed to read secure random bytes from OS RNG")?;
    Ok(buf[0])
}

fn encode_base62(bytes: &[u8]) -> String {
    const BASE62: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";

    if bytes.is_empty() {
        return String::new();
    }

    let zeros = bytes.iter().take_while(|&&b| b == 0).count();
    let mut input = bytes.to_vec();
    let mut encoded = Vec::new();
    let mut start = zeros;

    while start < input.len() {
        let mut remainder: u32 = 0;
        for value in &mut input[start..] {
            let acc = (remainder << 8) | (*value as u32);
            *value = (acc / 62) as u8;
            remainder = acc % 62;
        }
        encoded.push(BASE62[remainder as usize] as char);
        while start < input.len() && input[start] == 0 {
            start += 1;
        }
    }

    for _ in 0..zeros {
        encoded.push('0');
    }

    if encoded.is_empty() {
        return "0".to_string();
    }

    encoded.iter().rev().collect()
}
