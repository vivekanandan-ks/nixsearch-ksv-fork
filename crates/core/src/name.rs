use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NameParts {
    pub segments: Vec<String>,
    pub root: Option<String>,
    pub groups: Vec<String>,
    pub leaf: Option<String>,
}

impl NameParts {
    pub fn from_dotted(name: &str) -> Self {
        let segments: Vec<String> = name
            .split('.')
            .filter(|part| !part.is_empty())
            .map(ToOwned::to_owned)
            .collect();

        let root = if segments.len() > 1 {
            segments.first().cloned()
        } else {
            None
        };

        let groups = if segments.len() <= 1 {
            Vec::new()
        } else {
            (1..segments.len())
                .map(|end| segments[..end].join("."))
                .collect()
        };

        let leaf = segments.last().cloned();

        Self {
            segments,
            root,
            groups,
            leaf,
        }
    }
}

pub fn make_document_id(source: &str, ref_id: &str, kind: &str, name: &str) -> String {
    format!(
        "{}/{}/{}/{}",
        escape_id_part(source),
        escape_id_part(ref_id),
        escape_id_part(kind),
        escape_id_part(name),
    )
}

fn escape_id_part(part: &str) -> String {
    part.replace('/', "%2F")
}

#[cfg(test)]
mod tests {
    use super::{NameParts, make_document_id};

    #[test]
    fn document_ids_are_stable_and_include_identity_dimensions() {
        let id = make_document_id("nixos", "unstable", "option", "programs.git.enable");

        assert_eq!(id, "nixos/unstable/option/programs.git.enable");
    }

    #[test]
    fn document_id_parts_escape_slashes() {
        let id = make_document_id(
            "my/project",
            "release/25.05",
            "option",
            "programs.git.enable",
        );

        assert_eq!(
            id,
            "my%2Fproject/release%2F25.05/option/programs.git.enable"
        );
    }

    #[test]
    fn name_parts_for_nested_dotted_name() {
        let parts = NameParts::from_dotted("services.tailscale.enable");

        assert_eq!(parts.segments, ["services", "tailscale", "enable"]);
        assert_eq!(parts.root.as_deref(), Some("services"));
        assert_eq!(parts.groups, ["services", "services.tailscale"]);
        assert_eq!(parts.leaf.as_deref(), Some("enable"));
    }

    #[test]
    fn name_parts_for_package_set_name() {
        let parts = NameParts::from_dotted("rubyPackages.rails");

        assert_eq!(parts.segments, ["rubyPackages", "rails"]);
        assert_eq!(parts.root.as_deref(), Some("rubyPackages"));
        assert_eq!(parts.groups, ["rubyPackages"]);
        assert_eq!(parts.leaf.as_deref(), Some("rails"));
    }

    #[test]
    fn name_parts_for_single_name() {
        let parts = NameParts::from_dotted("git");

        assert_eq!(parts.segments, ["git"]);
        assert_eq!(parts.root, None);
        assert!(parts.groups.is_empty());
        assert_eq!(parts.leaf.as_deref(), Some("git"));
    }

    #[test]
    fn name_parts_ignores_empty_segments() {
        let parts = NameParts::from_dotted(".services..tailscale.enable.");

        assert_eq!(parts.segments, ["services", "tailscale", "enable"]);
        assert_eq!(parts.root.as_deref(), Some("services"));
        assert_eq!(parts.groups, ["services", "services.tailscale"]);
        assert_eq!(parts.leaf.as_deref(), Some("enable"));
    }
}
