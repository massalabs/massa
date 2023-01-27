use massa_api_exports::page::PageRequest;
use serde::{Deserialize, Serialize};

/// Wrap request params into struct for ApiV2 method
#[derive(Deserialize, Serialize)]
pub struct ApiRequest {
    pub page_request: Option<PageRequest>,
}
