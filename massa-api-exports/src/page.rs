// Copyright (c) 2022 MASSA LABS <info@massa.net>

use paginate::Pages;
use serde::{Deserialize, Serialize, Serializer};

/// Represents a Vec that can be split across Pages
/// Cf. <https://docs.rs/paginate/latest/paginate/>
#[derive(Clone)]
pub struct PagedVec<T> {
    res: Vec<T>,
    _total_count: usize,
}

impl<T: Serialize> PagedVec<T> {
    /// Creates a new Paged Vec with optional limits of item per page and offset
    pub fn new(elements: Vec<T>, page_request: Option<PageRequest>) -> Self {
        let total_count = elements.len();

        let (limit, offset) = match page_request {
            Some(PageRequest { limit, offset }) => (limit, offset),
            None => (total_count, 0),
        };

        let pages = Pages::new(total_count, limit);
        let page = pages.with_offset(offset);

        let res: Vec<_> = elements
            .into_iter()
            .skip(page.start)
            .take(page.length)
            .collect();

        PagedVec {
            res,
            _total_count: total_count,
        }
    }
}

impl<T: Serialize> Serialize for PagedVec<T> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.res.serialize::<S>(s)
    }
}

/// Represents the request inputs for a PagedVec
#[derive(Deserialize, Serialize)]
pub struct PageRequest {
    /// The limit of elements in a page
    pub limit: usize,
    /// The page offset
    pub offset: usize,
}

/// Represents the request inputs for a PagedVecV2
#[derive(Clone, Deserialize, Serialize)]
pub struct PagedVecV2<T> {
    content: Vec<T>,
    total_count: usize,
}

impl<T> From<PagedVec<T>> for PagedVecV2<T> {
    fn from(paged_vec: PagedVec<T>) -> Self {
        PagedVecV2 {
            content: paged_vec.res,
            total_count: paged_vec._total_count,
        }
    }
}
