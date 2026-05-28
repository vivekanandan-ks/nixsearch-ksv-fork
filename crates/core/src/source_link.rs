use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Repo {
    pub host: RepoHost,
    pub owner: String,
    pub repo: String,
    pub revision: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RepoHost {
    Github,
    Gitlab,
    Sourcehut,
    Codeberg,
    Custom(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Declaration {
    pub name: String,
    pub url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum SourceLinkConfig {
    Github {
        owner: String,
        repo: String,
        #[serde(default)]
        revision: Option<String>,
        #[serde(default)]
        strip_prefixes: Vec<String>,
    },

    Prefix {
        url_prefix: String,
        #[serde(default)]
        strip_prefixes: Vec<String>,
    },
}

#[derive(Debug, Clone, Copy)]
pub struct SourceLinkResolver<'a> {
    config: &'a SourceLinkConfig,
    revision: Option<&'a str>,
}

impl<'a> SourceLinkResolver<'a> {
    pub fn new(config: &'a SourceLinkConfig, revision: Option<&'a str>) -> Self {
        Self { config, revision }
    }

    pub fn resolve_declaration(&self, declaration: &Declaration) -> Option<String> {
        if let Some(url) = &declaration.url {
            return Some(url.clone());
        }

        self.resolve_location(&declaration.name)
    }

    pub fn resolve_package_position(&self, position: &str) -> Option<String> {
        self.resolve_location(position)
    }

    pub fn resolve_location(&self, location: &str) -> Option<String> {
        if is_absolute_url(location) {
            return Some(location.to_owned());
        }

        let normalized = normalize_source_location(location, self.config.strip_prefixes())?;
        self.url_for_path(&normalized.path, normalized.line.as_deref())
    }

    fn url_for_path(&self, path: &str, line: Option<&str>) -> Option<String> {
        let path = path.trim_start_matches('/');

        if path.is_empty() {
            return None;
        }

        let mut url = match self.config {
            SourceLinkConfig::Github {
                owner,
                repo,
                revision,
                ..
            } => {
                let revision = self.revision.or(revision.as_deref()).unwrap_or("master");

                format!(
                    "https://github.com/{owner}/{repo}/blob/{revision}/{path}",
                    owner = owner,
                    repo = repo,
                    revision = revision,
                    path = path
                )
            }

            SourceLinkConfig::Prefix { url_prefix, .. } => {
                format!("{}/{}", url_prefix.trim_end_matches('/'), path)
            }
        };

        if let Some(line) = line {
            url.push_str("#L");
            url.push_str(line);
        }

        Some(url)
    }
}

impl SourceLinkConfig {
    fn strip_prefixes(&self) -> &[String] {
        match self {
            Self::Github { strip_prefixes, .. } | Self::Prefix { strip_prefixes, .. } => {
                strip_prefixes
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NormalizedSourceLocation {
    path: String,
    line: Option<String>,
}

fn normalize_source_location(
    location: &str,
    strip_prefixes: &[String],
) -> Option<NormalizedSourceLocation> {
    let mut raw = location.trim();

    if raw.is_empty() {
        return None;
    }

    if raw.starts_with('<') && raw.ends_with('>') {
        raw = &raw[1..raw.len() - 1];
    }

    let (raw_path, line) = split_line_suffix(raw);
    let mut path = raw_path.to_owned();

    path = apply_strip_prefixes(&path, strip_prefixes);
    path = strip_nix_store_source_prefix(&path).unwrap_or(path);

    if let Some(rest) = path.strip_prefix("nixpkgs/") {
        path = rest.to_owned();
    }

    if path.starts_with('/') {
        return None;
    }

    let path = path.trim_start_matches("./").to_owned();

    if path.is_empty() {
        return None;
    }

    Some(NormalizedSourceLocation { path, line })
}

fn split_line_suffix(value: &str) -> (&str, Option<String>) {
    let Some((path, line)) = value.rsplit_once(':') else {
        return (value, None);
    };

    if line.chars().all(|ch| ch.is_ascii_digit()) {
        (path, Some(line.to_owned()))
    } else {
        (value, None)
    }
}

fn apply_strip_prefixes(path: &str, strip_prefixes: &[String]) -> String {
    for prefix in strip_prefixes {
        let prefix = prefix.trim();

        if prefix.is_empty() {
            continue;
        }

        if let Some(rest) = path.strip_prefix(prefix) {
            return rest.trim_start_matches('/').to_owned();
        }
    }

    path.to_owned()
}

fn strip_nix_store_source_prefix(path: &str) -> Option<String> {
    let rest = path.strip_prefix("/nix/store/")?;
    let (_, relative) = rest.split_once('/')?;

    if relative.is_empty() {
        None
    } else {
        Some(relative.to_owned())
    }
}

fn is_absolute_url(value: &str) -> bool {
    value.starts_with("http://") || value.starts_with("https://")
}

#[cfg(test)]
mod tests {
    use super::{Declaration, SourceLinkConfig, SourceLinkResolver};

    #[test]
    fn github_source_link_resolves_relative_declaration_with_line() {
        let config = SourceLinkConfig::Github {
            owner: "NixOS".into(),
            repo: "nixpkgs".into(),
            revision: Some("abc123".into()),
            strip_prefixes: Vec::new(),
        };

        let resolver = SourceLinkResolver::new(&config, None);

        let declaration = Declaration {
            name: "nixos/modules/programs/git.nix:42".into(),
            url: None,
        };

        assert_eq!(
            resolver.resolve_declaration(&declaration).as_deref(),
            Some("https://github.com/NixOS/nixpkgs/blob/abc123/nixos/modules/programs/git.nix#L42")
        );
    }

    #[test]
    fn github_source_link_uses_document_revision_as_fallback() {
        let config = SourceLinkConfig::Github {
            owner: "NixOS".into(),
            repo: "nixpkgs".into(),
            revision: None,
            strip_prefixes: Vec::new(),
        };

        let resolver = SourceLinkResolver::new(&config, Some("def456"));

        assert_eq!(
            resolver
                .resolve_location("pkgs/applications/version-management/git/default.nix")
                .as_deref(),
            Some(
                "https://github.com/NixOS/nixpkgs/blob/def456/pkgs/applications/version-management/git/default.nix"
            )
        );
    }

    #[test]
    fn prefix_source_link_resolves_relative_location() {
        let config = SourceLinkConfig::Prefix {
            url_prefix: "https://example.com/blob/main/".into(),
            strip_prefixes: Vec::new(),
        };

        let resolver = SourceLinkResolver::new(&config, None);

        assert_eq!(
            resolver.resolve_location("modules/foo.nix:7").as_deref(),
            Some("https://example.com/blob/main/modules/foo.nix#L7")
        );
    }

    #[test]
    fn source_link_preserves_existing_declaration_url() {
        let config = SourceLinkConfig::Prefix {
            url_prefix: "https://example.com/blob/main/".into(),
            strip_prefixes: Vec::new(),
        };

        let resolver = SourceLinkResolver::new(&config, None);

        let declaration = Declaration {
            name: "foo.nix".into(),
            url: Some("https://already.example/source.nix".into()),
        };

        assert_eq!(
            resolver.resolve_declaration(&declaration).as_deref(),
            Some("https://already.example/source.nix")
        );
    }

    #[test]
    fn source_link_preserves_absolute_location_url() {
        let config = SourceLinkConfig::Prefix {
            url_prefix: "https://example.com/blob/main/".into(),
            strip_prefixes: Vec::new(),
        };

        let resolver = SourceLinkResolver::new(&config, None);

        assert_eq!(
            resolver
                .resolve_location("https://already.example/source.nix#L1")
                .as_deref(),
            Some("https://already.example/source.nix#L1")
        );
    }

    #[test]
    fn source_link_strips_nix_store_source_prefix() {
        let config = SourceLinkConfig::Github {
            owner: "example".into(),
            repo: "repo".into(),
            revision: Some("main".into()),
            strip_prefixes: Vec::new(),
        };

        let resolver = SourceLinkResolver::new(&config, None);

        assert_eq!(
            resolver
                .resolve_location("/nix/store/abc123-source/modules/foo.nix:9")
                .as_deref(),
            Some("https://github.com/example/repo/blob/main/modules/foo.nix#L9")
        );
    }

    #[test]
    fn source_link_applies_configured_strip_prefixes() {
        let config = SourceLinkConfig::Github {
            owner: "example".into(),
            repo: "repo".into(),
            revision: Some("main".into()),
            strip_prefixes: vec!["/build/source/".into()],
        };

        let resolver = SourceLinkResolver::new(&config, None);

        assert_eq!(
            resolver
                .resolve_location("/build/source/modules/foo.nix")
                .as_deref(),
            Some("https://github.com/example/repo/blob/main/modules/foo.nix")
        );
    }

    #[test]
    fn source_link_strips_nixpkgs_angle_prefix() {
        let config = SourceLinkConfig::Github {
            owner: "NixOS".into(),
            repo: "nixpkgs".into(),
            revision: Some("main".into()),
            strip_prefixes: Vec::new(),
        };

        let resolver = SourceLinkResolver::new(&config, None);

        assert_eq!(
            resolver
                .resolve_location("<nixpkgs/nixos/modules/foo.nix>")
                .as_deref(),
            Some("https://github.com/NixOS/nixpkgs/blob/main/nixos/modules/foo.nix")
        );
    }

    #[test]
    fn source_link_returns_none_for_unstripped_absolute_paths() {
        let config = SourceLinkConfig::Github {
            owner: "example".into(),
            repo: "repo".into(),
            revision: Some("main".into()),
            strip_prefixes: Vec::new(),
        };

        let resolver = SourceLinkResolver::new(&config, None);

        assert_eq!(
            resolver.resolve_location("/some/random/local/path.nix"),
            None
        );
    }

    #[test]
    fn github_source_link_prefers_document_revision_over_config_revision() {
        let config = SourceLinkConfig::Github {
            owner: "NixOS".into(),
            repo: "nixpkgs".into(),
            revision: Some("branch-name".into()),
            strip_prefixes: Vec::new(),
        };

        let resolver = SourceLinkResolver::new(&config, Some("exact-commit"));

        assert_eq!(
            resolver
                .resolve_location("nixos/modules/foo.nix")
                .as_deref(),
            Some("https://github.com/NixOS/nixpkgs/blob/exact-commit/nixos/modules/foo.nix")
        );
    }
}
