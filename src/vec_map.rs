#![allow(dead_code)]

use std::ops::RangeTo;

pub struct VecMap<T>(Vec<T>);

impl<T: std::fmt::Debug> std::fmt::Debug for VecMap<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
impl<T> Default for VecMap<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> VecMap<T> {
    pub fn new() -> Self {
        VecMap(Vec::new())
    }
    pub fn with_capacity(capacity: usize) -> Self {
        VecMap(Vec::with_capacity(capacity))
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn inner_mut(&mut self) -> &mut Vec<T> {
        &mut self.0
    }
}

impl<T: Default> VecMap<T> {
    pub fn get_existing(&self, i: usize) -> &T {
        &self.0[i]
    }

    pub fn get_mut(&mut self, i: usize) -> &mut T {
        self.ensure_space_for(i);
        &mut self.0[i]
    }

    fn ensure_space_for(&mut self, i: usize) {
        let new_len = i + 1;
        if new_len > self.0.len() {
            self.0.resize_with(new_len, T::default);
        }
    }
}

impl<T: Default + Copy> VecMap<T> {
    pub fn set_all(&mut self, to: RangeTo<usize>, val: T) {
        if let Some(end_inclusive) = to.end.checked_sub(1) {
            self.ensure_space_for(end_inclusive);

            for i in 0..to.end {
                self.0[i] = val;
            }
        }
    }
}

impl<T> IntoIterator for VecMap<T> {
    type Item = T;
    type IntoIter = <Vec<T> as IntoIterator>::IntoIter;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<'a, T> IntoIterator for &'a VecMap<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}
