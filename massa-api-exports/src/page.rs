// Copyright (c) 2022 MASSA LABS <info@massa.net>

use paginate::Pages;
use serde::{Deserialize, Serialize};

/// Represents a Vec that can be split across Pages
/// Cf. https://docs.rs/paginate/latest/paginate/
pub struct PagedVec<T> {
    res: Vec<T>,
    total_count: usize,
}

impl<T: Serialize> PagedVec<T> {
    /// Creates a new Paged Vec with optional limits of item per page and offset
    pub fn new(elements: Vec<T>, page_request: Option<PageRequest>) -> Self {
        let total_count = elements.len();

        let (limit, offset) = match page_request {
            Some(PageRequest { limit, offset }) => (limit, offset),
            None => (total_count,0)
        };

        let pages = Pages::new(total_count, limit);
        let page = pages.with_offset(offset);

        let res: Vec<_> = elements
            .into_iter()
            .skip(page.start)
            .take(page.length)
            .collect();

        PagedVec { res, total_count }
    }
}

/// Represents the request inputs for a PagedVec
#[derive(Deserialize)]
pub struct PageRequest {
    /// The limit of elements in a page
    pub limit: usize,
    /// The page offset
    pub offset: usize,
}

/*
impl PageRequest {
    /// Creates a new input request for a PagedVec
    pub fn new(limit: Option<usize>, offset: Option<usize>) -> Self {
        PageRequest {limit, offset}
    }
}*/
