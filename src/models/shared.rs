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
        write!(f, "Page: {} | Limit: {}", self.page, self.limit)?;

        if let Some(prev_page) = self.prev_page {
            write!(f, " | Prev Page: {}", prev_page)?;
        }

        if let Some(next_page) = self.next_page {
            write!(f, " | Next Page: {}", next_page)?;
        }

        write!(f, " | Total Pages: {}", self.total_pages)?;
        write!(f, " | Total Items: {}", self.total_items)?;

        Ok(())

        // write!(
        //     f,
        //     "Page: {} | Limit: {} | Next Page: {} | Prev Page: {} | Total Pages: {} |  Total Items: {} ",
        //     self.page,
        //     self.limit,
        //     self.next_page.map_or("None".to_string(), |n| n.to_string()),
        //     self.prev_page.map_or("None".to_string(), |p| p.to_string()),
        //     self.total_pages,
        //     self.total_items,
        // )
    }
}
