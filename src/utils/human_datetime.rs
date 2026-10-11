use anyhow::Result;
use chrono::{DateTime, SecondsFormat, Utc};
use chrono_humanize::HumanTime;
use log::debug;

/// Formats an instant the way every absolute date in the CLI is shown:
/// UTC, whole seconds, RFC 3339 with a `Z` (e.g. `2026-09-13T11:53:59Z`).
pub fn format_utc(date_time: DateTime<Utc>) -> String {
    date_time.to_rfc3339_opts(SecondsFormat::Secs, true)
}

/// Parses an RFC 3339 timestamp in any offset and re-formats it with
/// [`format_utc`].
pub fn format_utc_timestamp(value: &str) -> Result<String> {
    Ok(format_utc(
        DateTime::parse_from_rfc3339(value)?.with_timezone(&Utc),
    ))
}

/// [`format_utc_timestamp`] for display: anything that doesn't parse
/// (including an empty "never" value) is shown unchanged.
pub fn display_utc_timestamp(value: &str) -> String {
    format_utc_timestamp(value).unwrap_or_else(|_| value.to_owned())
}

pub fn get_human_datetime(date_time_str: &str) -> (String, HumanTime) {
    match DateTime::parse_from_rfc3339(date_time_str) {
        Ok(d) => {
            let datetime = d.with_timezone(&Utc);
            let ht = HumanTime::from(datetime - Utc::now());
            (format_utc(datetime), ht)
        }
        Err(e) => {
            debug!("{:#?}", &e);
            (
                "Invalid datetime format".to_string(),
                HumanTime::from(chrono::Duration::zero()),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{display_utc_timestamp, format_utc_timestamp, get_human_datetime};

    #[test]
    fn timestamps_are_utc_seconds() {
        assert_eq!(
            format_utc_timestamp("2026-09-13T11:53:59.237435+00:00").unwrap(),
            "2026-09-13T11:53:59Z"
        );
        assert_eq!(
            format_utc_timestamp("2026-07-03T16:10:07+02:00").unwrap(),
            "2026-07-03T14:10:07Z"
        );
    }

    #[test]
    fn human_datetime_is_utc_regardless_of_local_timezone() {
        let (formatted, _) = get_human_datetime("2026-07-03T16:10:07.5+02:00");
        assert_eq!(formatted, "2026-07-03T14:10:07Z");
    }

    #[test]
    fn display_keeps_unparseable_values() {
        assert_eq!(
            display_utc_timestamp("2026-07-03T16:10:07+02:00"),
            "2026-07-03T14:10:07Z"
        );
        assert_eq!(display_utc_timestamp(""), "");
    }
}
