use std::collections::HashMap;

use crate::agent_mgmt::builtin;
use crate::agent_mgmt::recipe::Recipe;

/// Where a resolved recipe came from. Reported back to status callers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RecipeSource {
    Builtin,
    Config,
    ControlPlane,
}

pub struct RecipeRegistry {
    config: HashMap<String, Recipe>,
    builtin: HashMap<String, Recipe>,
}

impl RecipeRegistry {
    pub fn new(config: HashMap<String, Recipe>) -> Self {
        Self {
            config,
            builtin: builtin::builtins(),
        }
    }

    /// Resolve a recipe by name, applying any ephemeral (per-command) override
    /// first. Returns the recipe and where it came from.
    pub fn resolve(
        &self,
        name: &str,
        ephemeral: Option<&Recipe>,
    ) -> Option<(Recipe, RecipeSource)> {
        if let Some(r) = ephemeral {
            return Some((r.clone(), RecipeSource::ControlPlane));
        }
        if let Some(r) = self.config.get(name) {
            return Some((r.clone(), RecipeSource::Config));
        }
        if let Some(r) = self.builtin.get(name) {
            return Some((r.clone(), RecipeSource::Builtin));
        }
        None
    }

    /// All recipe names known to the registry (builtin ∪ config). Used by
    /// `List`/`Status` callers.
    pub fn known_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .builtin
            .keys()
            .chain(self.config.keys())
            .cloned()
            .collect();
        names.sort();
        names.dedup();
        names
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_mgmt::recipe::{ArchiveKind, Recipe, RecipeSource as RS};

    fn url_recipe(name: &str, url: &str) -> Recipe {
        Recipe {
            name: name.to_string(),
            display_name: name.to_string(),
            source: RS::Url {
                template: url.to_string(),
                archive: ArchiveKind::TarGz,
                checksum: None,
            },
            entry_points: vec![],
            version_check: None,
            post_install: vec![],
            pre_remove: vec![],
        }
    }

    #[test]
    fn ephemeral_overrides_config_and_builtin() {
        let reg = RecipeRegistry::new(HashMap::from([(
            "hermes-agent".to_string(),
            url_recipe("hermes-agent", "https://config/{version}.tgz"),
        )]));

        let ephemeral = url_recipe("hermes-agent", "https://ctrl/{version}.tgz");
        let (r, src) = reg.resolve("hermes-agent", Some(&ephemeral)).unwrap();
        assert_eq!(src, RecipeSource::ControlPlane);
        match r.source {
            RS::Url { template, .. } => assert!(template.contains("ctrl")),
            _ => panic!("expected url"),
        }
    }

    #[test]
    fn config_overrides_builtin() {
        let reg = RecipeRegistry::new(HashMap::from([(
            "hermes-agent".to_string(),
            url_recipe("hermes-agent", "https://config/{version}.tgz"),
        )]));
        let (_, src) = reg.resolve("hermes-agent", None).unwrap();
        assert_eq!(src, RecipeSource::Config);
    }

    #[test]
    fn builtin_used_when_nothing_else() {
        let reg = RecipeRegistry::new(HashMap::new());
        let (_, src) = reg.resolve("hermes-agent", None).unwrap();
        assert_eq!(src, RecipeSource::Builtin);
    }
}
