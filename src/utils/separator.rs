#[derive(Debug)]
pub struct KeyValueError;

impl std::fmt::Display for KeyValueError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Expected a key-value pair (separated by '=').")
    }
}

pub fn key_value(values: Vec<String>) -> Result<Vec<(String, String)>, KeyValueError> {
    let mut key_value_vec: Vec<_> = Vec::new();

    for val in values.iter() {
        match val.split_once("=") {
            Some((key, value)) => {
                // if key.is_empty() || value.is_empty() {
                if key.is_empty() {
                    return Err(KeyValueError);
                }
                //ok
                let val = (format!("{}", key), format!("{}", value));
                key_value_vec.push(val);
            }
            None => {
                return Err(KeyValueError);
            }
        }
    }

    Ok(key_value_vec)
}
