use crate::agent_mgmt::recipe::{
    ArchiveKind, EntryPoint, Recipe, RecipeSource, VersionCheck,
};

/// Built-in recipe for hermes-agent.
///
/// TODO(operator): fill in the real download URL once hermes-agent's release
/// process is settled. While `template` is empty the recipe reports
/// "not yet configured" and the installer refuses to run; operators can
/// override the recipe in `[agent_management.recipes.hermes-agent]` in
/// config.toml without rebuilding the worker.
pub fn recipe() -> Recipe {
    Recipe {
        name: "hermes-agent".to_string(),
        display_name: "Hermes Agent".to_string(),
        source: RecipeSource::Url {
            template: String::new(),
            archive: ArchiveKind::TarGz,
            checksum: None,
        },
        entry_points: vec![EntryPoint {
            // The upstream binary is just `hermes`. Inside our managed
            // payload it lives at the root of the extracted archive.
            relative_path: "hermes".to_string(),
            symlink_name: "hermes".to_string(),
            // Where the upstream installer drops the binary when run by
            // hand. Lets us mark hermes "installed" without requiring it
            // to be on `$PATH`.
            discovery_paths: vec![
                "~/.hermes/hermes-agent/hermes".to_string(),
                "~/.hermes/bin/hermes".to_string(),
            ],
        }],
        version_check: Some(VersionCheck {
            args: vec!["--version".to_string()],
            regex: None,
        }),
        post_install: vec![],
        pre_remove: vec![],
    }
}
