//! Keyboard layout mapping utilities.

use std::collections::HashMap;

/// Maps physical key codes to logical characters for a given layout.
#[derive(Debug)]
pub struct KeyMap {
    map: HashMap<char, char>,
}

impl KeyMap {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    pub fn insert(&mut self, from: char, to: char) {
        self.map.insert(from, to);
    }

    pub fn get(&self, key: &char) -> Option<&char> {
        self.map.get(key)
    }
}

impl Default for KeyMap {
    fn default() -> Self {
        Self::new()
    }
}
