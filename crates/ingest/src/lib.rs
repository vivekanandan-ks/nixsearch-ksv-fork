use std::io::Read;

use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::Value;

use nix_search_core::{Declaration, IngestContext, OptionDoc, SearchDocument};

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawDeclaration {
    Path(String),
    Link {
        name: String,
        #[serde(default)]
        url: Option<String>,
    },
}

impl From<RawDeclaration> for Declaration {
    fn from(value: RawDeclaration) -> Self {
        match value {
            RawDeclaration::Path(path) => Self {
                name: path,
                url: None,
            },
            RawDeclaration::Link { name, url } => Self { name, url },
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawOption {
    #[serde(default)]
    declarations: Vec<RawDeclaration>,

    #[serde(default)]
    description: Option<String>,

    #[serde(default, rename = "type")]
    option_type: Option<String>,

    #[serde(default)]
    default: Option<Value>,

    #[serde(default)]
    example: Option<Value>,

    #[serde(default)]
    related_packages: Option<String>,

    #[serde(default)]
    loc: Vec<String>,

    #[serde(default)]
    read_only: Option<bool>,

    #[serde(default)]
    internal: Option<bool>,

    #[serde(default)]
    visible: Option<bool>,
}

pub fn parse_options_json<R: Read>(
    reader: R,
    context: &IngestContext,
) -> Result<Vec<SearchDocument>> {
    let raw_options: serde_json::Map<String, Value> =
        serde_json::from_reader(reader).context("failed to parse options JSON")?;

    let mut documents = Vec::with_capacity(raw_options.len());

    for (name, value) in raw_options {
        let raw: RawOption = serde_json::from_value(value)
            .with_context(|| format!("failed to parse option {name}"))?;

        documents.push(SearchDocument::Option(convert_option(name, raw, context)));
    }

    documents.sort_by(|left, right| left.id().cmp(right.id()));

    Ok(documents)
}

fn convert_option(name: String, raw: RawOption, context: &IngestContext) -> OptionDoc {
    let mut doc = OptionDoc::new(context, name);

    doc.loc = if raw.loc.is_empty() {
        doc.common.name.split('.').map(ToOwned::to_owned).collect()
    } else {
        raw.loc
    };

    doc.parents = parents_from_loc(&doc.loc);
    doc.option_set = doc.loc.first().cloned();
    doc.declarations = raw.declarations.into_iter().map(Into::into).collect();
    doc.description = raw.description;
    doc.option_type = raw.option_type;
    doc.default = raw.default;
    doc.example = raw.example;
    doc.related_packages = raw.related_packages;
    doc.read_only = raw.read_only;
    doc.internal = raw.internal;
    doc.visible = raw.visible;

    doc
}

fn parents_from_loc(loc: &[String]) -> Vec<String> {
    if loc.len() <= 1 {
        return Vec::new();
    }

    (1..loc.len()).map(|end| loc[..end].join(".")).collect()
}

#[cfg(test)]
mod tests {
    use nix_search_core::{IngestContext, SearchDocument};

    use super::parse_options_json;

    fn test_context() -> IngestContext {
        IngestContext {
            project: "nixpkgs".into(),
            dataset: "nixos-options".into(),
            ref_id: "unstable".into(),
            revision: None,
            repo: None,
        }
    }

    #[test]
    fn parses_basic_options_json() {
        let json = r#"
           {
             "programs.git.enable": {
               "declarations": ["/source/modules/programs/git.nix"],
               "description": "Whether to enable Git.",
               "type": "boolean",
               "default": { "_type": "literalExpression", "text": "false" },
               "example": true,
               "loc": ["programs", "git", "enable"],
               "visible": true
             }
           }
           "#;

        let docs = parse_options_json(json.as_bytes(), &test_context()).unwrap();

        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0].name(), "programs.git.enable");

        let SearchDocument::Option(option) = &docs[0];

        assert_eq!(option.common.project, "nixpkgs");
        assert_eq!(option.common.dataset, "nixos-options");
        assert_eq!(option.common.ref_id, "unstable");
        assert_eq!(option.loc, ["programs", "git", "enable"]);
        assert_eq!(option.option_set.as_deref(), Some("programs"));
        assert_eq!(option.option_type.as_deref(), Some("boolean"));
        assert_eq!(
            option.description.as_deref(),
            Some("Whether to enable Git.")
        );
        assert_eq!(option.visible, Some(true));
        assert_eq!(option.declarations.len(), 1);
        assert_eq!(
            option.declarations[0].name,
            "/source/modules/programs/git.nix"
        );
    }

    #[test]
    fn derives_parents_from_loc() {
        let json = r#"
           {
             "boot.loader.systemd-boot.enable": {
               "description": "Whether to enable systemd-boot.",
               "loc": ["boot", "loader", "systemd-boot", "enable"]
             }
           }
           "#;

        let docs = parse_options_json(json.as_bytes(), &test_context()).unwrap();

        let SearchDocument::Option(option) = &docs[0];

        assert_eq!(
            option.parents,
            ["boot", "boot.loader", "boot.loader.systemd-boot"]
        );
        assert_eq!(option.option_set.as_deref(), Some("boot"));
    }

    #[test]
    fn falls_back_to_name_segments_when_loc_is_missing() {
        let json = r#"
           {
             "services.nginx.enable": {
               "description": "Whether to enable Nginx."
             }
           }
           "#;

        let docs = parse_options_json(json.as_bytes(), &test_context()).unwrap();

        let SearchDocument::Option(option) = &docs[0];

        assert_eq!(option.loc, ["services", "nginx", "enable"]);
        assert_eq!(option.parents, ["services", "services.nginx"]);
        assert_eq!(option.option_set.as_deref(), Some("services"));
    }

    #[test]
    fn parses_link_declarations() {
        let json = r#"
           {
             "programs.git.enable": {
               "declarations": [
                 {
                   "name": "git.nix",
                   "url": "https://github.com/NixOS/nixpkgs/blob/master/nixos/modules/programs/git.nix"
                 }
               ],
               "description": "Whether to enable Git."
             }
           }
           "#;

        let docs = parse_options_json(json.as_bytes(), &test_context()).unwrap();

        let SearchDocument::Option(option) = &docs[0];

        assert_eq!(option.declarations.len(), 1);
        assert_eq!(option.declarations[0].name, "git.nix");
        assert_eq!(
            option.declarations[0].url.as_deref(),
            Some("https://github.com/NixOS/nixpkgs/blob/master/nixos/modules/programs/git.nix")
        );
    }

    #[test]
    fn sorts_documents_by_stable_id() {
        let json = r#"
           {
             "services.nginx.enable": {
               "description": "Whether to enable Nginx."
             },
             "programs.git.enable": {
               "description": "Whether to enable Git."
             }
           }
           "#;

        let docs = parse_options_json(json.as_bytes(), &test_context()).unwrap();

        assert_eq!(docs.len(), 2);
        assert_eq!(docs[0].name(), "programs.git.enable");
        assert_eq!(docs[1].name(), "services.nginx.enable");
    }
}
