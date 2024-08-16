use core::fmt;

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

// Implement the Display trait for PaginationMetadata
impl fmt::Display for PaginationMetadata {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "------- Pagination Metadata -------")?;

        write!(
            f,
            "Page: {} | Limit: {} | Next Page: {} | Prev Page: {} | Total Pages: {} |  Total Items: {} ",
            self.page,
            self.limit,
            self.next_page.map_or("None".to_string(), |n| n.to_string()),
            self.prev_page.map_or("None".to_string(), |p| p.to_string()),
            self.total_items,
            self.total_pages,
        )
    }
}
