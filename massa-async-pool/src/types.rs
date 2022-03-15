//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! Provides tools to represent and manipulate changes happening on the async message pool.

/// Enum representing a add/delete change on a value T
#[derive(Debug, Clone)]
pub enum AddOrDelete<T> {
    /// adds a new value T
    Add(T),

    /// deletes the value T
    Delete,
}

/// allows applying another AddOrDelete to the current one
impl<T> AddOrDelete<T> {
    fn apply(&mut self, other: Self) {
        *self = other;
    }
}
