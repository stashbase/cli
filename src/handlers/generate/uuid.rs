use uuid::Uuid;

use crate::utils::output::get_formatted_json_string;

pub fn handle_generate_uuid_v4(json_format: bool) {
    let uuid = Uuid::new_v4();

    if json_format {
        let json = serde_json::json!({ "value": uuid.to_string() });
        let json_pretty = get_formatted_json_string(&json, true).unwrap();
        println!("{}", json_pretty);
    } else {
        println!("{}", uuid.to_string());
    }
}

pub fn handle_generate_uuid_v7(json_format: bool) {
    let timestamp = uuid::Timestamp::now(uuid::timestamp::context::Context::new(0));
    let uuid = Uuid::new_v7(timestamp);

    if json_format {
        let json = serde_json::json!({ "value": uuid.to_string() });
        let json_pretty = get_formatted_json_string(&json, true).unwrap();
        println!("{}", json_pretty);
    } else {
        println!("{}", uuid.to_string());
    }
}
