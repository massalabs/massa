// Copyright (c) 2022 MASSA LABS <info@massa.net>

use paginate::Pages;
use serde::{Deserialize, Serialize};

/// Represents a Vec that can be split across Pages
/// Cf. https://docs.rs/paginate/latest/paginate/
#[derive(Serialize)]
pub struct PagedVec<T> {
    res: Vec<T>,
    total_count: usize,
}

impl<T: Serialize> PagedVec<T> {
    /// Creates a new Paged Vec with optional limits of item per page and offset
    pub fn new(elements: Vec<T>, limit: Option<usize>, offset: Option<usize>) -> Self {
        let total_count = elements.len();

        let pages = Pages::new(total_count, limit.unwrap_or(total_count));
        let page = pages.with_offset(offset.unwrap_or_default());

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
    pub limit: Option<usize>,
    /// The page offset
    pub offset: Option<usize>,
}

/*
impl PageRequest {
    /// Creates a new input request for a PagedVec
    pub fn new(limit: Option<usize>, offset: Option<usize>) -> Self {
        PageRequest {limit, offset}
    }
}*/