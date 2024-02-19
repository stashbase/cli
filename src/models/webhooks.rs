use serde::{Deserialize, Serialize};
use tabled::Tabled;

#[derive(Debug, Serialize, Deserialize, Tabled)]
#[serde(rename_all = "camelCase")]
pub struct ListWebhook {
    #[tabled(order = 0)]
    id: String,

    #[tabled(order = 1)]
    url: String,

    #[tabled(order = 2)]
    enabled: bool,
}
