use std::collections::HashMap;

use crate::agent_mgmt::recipe::Recipe;

pub mod hermes;

/// All built-in recipes shipped with the worker. Keyed by recipe name.
pub fn builtins() -> HashMap<String, Recipe> {
    let mut map = HashMap::new();
    let h = hermes::recipe();
    map.insert(h.name.clone(), h);
    map
}
