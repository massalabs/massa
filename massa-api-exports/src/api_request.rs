use crate::page::PageRequest;
use serde::{Deserialize, Serialize};

/// Wrap request params into struct for ApiV2 method
#[derive(Deserialize, Serialize)]
pub struct ApiRequest {
    /// pagination
    pub page_request: Option<PageRequest>,
}
