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
    use nix_search_core::IngestContext;

    use super::parse_options_json;

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

        let context = IngestContext {
            project: "nixpkgs".into(),
            dataset: "nixos-options".into(),
            ref_id: "unstable".into(),
            revision: None,
            repo: None,
        };

        let docs = parse_options_json(json.as_bytes(), &context).unwrap();

        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0].name(), "programs.git.enable");
    }
}

