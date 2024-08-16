use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaginationMetadata {
    // current page
    pub page: usize,
    // the number of items per page
    pub limit: usize,
    // the total number of items
    pub total_items: usize,
    // the total number of pages
    pub total_pages: usize,
    // number of the next page
    pub next_page: Option<usize>,
    // number of the previous page
    pub prev_page: Option<usize>,
}
