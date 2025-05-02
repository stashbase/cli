#[derive(Debug)]
pub struct KeyValueError;

// TODO
impl std::fmt::Display for KeyValueError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Expected a key-value pair (separated by '=').")
    }
}

// TODO: imput error
pub fn key_value(values: Vec<String>) -> Result<Vec<(String, String)>, KeyValueError> {
    let mut key_value_vec: Vec<_> = Vec::new();

    for val in values.iter() {
        // let split_index = val.find("=");
        // match split_index {
        //     Some(index) => {
        //         let (key, value) = val.split_at(index);
        //
        //         let val = (format!("{}", key), format!("{}", value.split_at(1).1));
        //         key_value_vec.push(val);
        //     }
        //     None => return Err(KeyValueError),
        // }

        // FIX: does not work if value with quotes
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
