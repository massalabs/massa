use crate::LedgerConfig;
use massa_hash::hash::Hash;
use massa_models::{prehash::Map, Address, Amount, Slot};
use std::collections::{BTreeMap, VecDeque};

/// represents a structure that supports another one being applied to it
pub trait Applicable<V> {
    fn apply(&mut self, _: V);
}

/// represents a set/update/delete change
#[derive(Debug, Clone)]
pub enum SetUpdateOrDelete<T: Default + Applicable<V>, V: Applicable<V> + Clone> {
    /// sets a new absolute value T
    Set(T),
    /// applies an update V to an existing value
    Update(V),
    /// deletes a value
    Delete,
}

/// supports applying another SetUpdateOrDelete to self
impl<T: Default + Applicable<V>, V: Applicable<V>> Applicable<SetUpdateOrDelete<T, V>>
    for SetUpdateOrDelete<T, V>
where
    V: Clone,
{
    fn apply(&mut self, other: SetUpdateOrDelete<T, V>) {
        match other {
            // the other SetUpdateOrDelete sets a new absolute value => force it on self
            v @ SetUpdateOrDelete::Set(_) => *self = v,

            // the other SetUpdateOrDelete updates the value
            SetUpdateOrDelete::Update(u) => match self {
                // if self currently sets an absolute value, apply other to that value
                SetUpdateOrDelete::Set(cur) => cur.apply(u),

                // if self currently updates a value, apply the updates of the other to that update
                SetUpdateOrDelete::Update(cur) => cur.apply(u),

                // if self currently deletes a value,
                // create a new default value, apply other's updates to it and make self set it as an absolute new value
                SetUpdateOrDelete::Delete => {
                    let mut res = T::default();
                    res.apply(u);
                    *self = SetUpdateOrDelete::Set(res);
                }
            },

            // the other SetUpdateOrDelete deletes a value => force self to delete it as well
            v @ SetUpdateOrDelete::Delete => *self = v,
        }
    }
}

/// represents a set/delete change
#[derive(Debug, Clone)]
pub enum SetOrDelete<T: Clone> {
    /// sets a new absolute value T
    Set(T),
    /// deletes a value
    Delete,
}

/// allows applying another SetOrDelete to the current one
impl<T: Clone> Applicable<SetOrDelete<T>> for SetOrDelete<T> {
    fn apply(&mut self, other: Self) {
        *self = other;
    }
}

/// represents a set/keep change
#[derive(Debug, Clone)]
pub enum SetOrKeep<T: Clone> {
    /// sets a new absolute value T
    Set(T),
    /// keeps the existing value
    Keep,
}

/// allows applying another SetOrKeep to the current one
impl<T: Clone> Applicable<SetOrKeep<T>> for SetOrKeep<T> {
    fn apply(&mut self, other: SetOrKeep<T>) {
        if let v @ SetOrKeep::Set(..) = other {
            // update the current value only if the other SetOrKeep sets a new one
            *self = v;
        }
    }
}

impl<T: Clone> SetOrKeep<T> {
    /// applies the current SetOrKeep into a target mutable value
    pub fn apply_to(self, val: &mut T) {
        if let SetOrKeep::Set(v) = self {
            // only change the value if self is setting a new one
            *val = v;
        }
    }
}

impl<T: Clone> Default for SetOrKeep<T> {
    fn default() -> Self {
        SetOrKeep::Keep
    }
}
