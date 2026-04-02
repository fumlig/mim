use anyhow::Result;
use chrono::Utc;
use chrono_tz::Tz;
use schemars::JsonSchema;
use serde::de::Error as _;
use serde::Deserialize;

use agent::tool::{function_tool, Tool};

pub fn make_tools() -> Result<Vec<Tool>> {
    #[derive(Deserialize, JsonSchema)]
    struct GetCurrentTimeParams {
        /// IANA time zone name, e.g. "America/New_York", "Europe/Berlin", "UTC"
        timezone: String,
    }

    let get_current_time = function_tool(
        "get_current_time".into(),
        "Get the current date and time in a given IANA time zone (e.g. \"America/New_York\", \"Europe/Berlin\", \"UTC\").".into(),
        |params: GetCurrentTimeParams| {
            let tz: Tz = params
                .timezone
                .parse()
                .map_err(|_| serde_json::Error::custom(format!("unknown timezone: {}", params.timezone)))?;
            let now = Utc::now().with_timezone(&tz);
            Ok(serde_json::json!({
                "timezone": params.timezone,
                "datetime": now.to_rfc3339(),
            }))
        },
    )?;

    Ok(vec![get_current_time])
}
