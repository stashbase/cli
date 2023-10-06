use chrono::prelude::DateTime;
use chrono::Local;
use chrono_humanize::HumanTime;

// pub fn get_human_datetime(date_time_str: String) -> (String, HumanTime) {
//
//     let datetime = DateTime::<Local>::from(d);
//
//     let date_time = datetime.format("%Y-%m-%d %H:%M").to_string();
//
//     let dt = datetime - chrono::Local::now();
//     let ht = chrono_humanize::HumanTime::from(dt);
//
//     return (date_time, ht);
// }
//
pub fn get_human_datetime(date_time_str: &str) -> (String, HumanTime) {
    if let Ok(d) = DateTime::parse_from_rfc3339(date_time_str) {
        let datetime = DateTime::<Local>::from(d);

        let date_time = datetime.format("%Y-%m-%d %H:%M").to_string();
        let dt = datetime - chrono::Local::now();

        let ht = HumanTime::from(dt);
        return (date_time, ht);
    } else {
        // Handle parsing error
        return (
            "Invalid datetime format".to_string(),
            HumanTime::from(chrono::Duration::zero()),
        );
    }
}
