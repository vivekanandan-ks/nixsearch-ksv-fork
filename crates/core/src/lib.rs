use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DocumentKind {
    Option,
    Package,
    App,
    Service,
}

impl DocumentKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Option => "option",
            Self::Package => "package",
            Self::App => "app",
            Self::Service => "service",
        }
    }
}

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
pub struct IngestContext {
    pub project: String,
    pub dataset: String,
    pub ref_id: String,
    pub revision: Option<String>,
    pub repo: Option<Repo>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommonDoc {
    pub id: String,
    pub project: String,
    pub dataset: String,
    pub ref_id: String,
    pub kind: DocumentKind,
    pub name: String,
    pub revision: Option<String>,
    pub repo: Option<Repo>,
    #[serde(with = "time::serde::rfc3339")]
    pub imported_at: OffsetDateTime,
}

impl CommonDoc {
    pub fn new(context: &IngestContext, kind: DocumentKind, name: impl Into<String>) -> Self {
        let name = name.into();
        let id = make_document_id(
            &context.project,
            &context.dataset,
            &context.ref_id,
            kind.as_str(),
            &name,
        );

        Self {
            id,
            project: context.project.clone(),
            dataset: context.dataset.clone(),
            ref_id: context.ref_id.clone(),
            kind,
            name,
            revision: context.revision.clone(),
            repo: context.repo.clone(),
            imported_at: OffsetDateTime::now_utc(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OptionDoc {
    #[serde(flatten)]
    pub common: CommonDoc,

    pub loc: Vec<String>,
    pub parents: Vec<String>,
    pub option_set: Option<String>,
    pub declarations: Vec<Declaration>,
    pub description: Option<String>,
    pub option_type: Option<String>,
    pub default: Option<Value>,
    pub example: Option<Value>,
    pub related_packages: Option<String>,
    pub read_only: Option<bool>,
    pub internal: Option<bool>,
    pub visible: Option<bool>,
}

impl OptionDoc {
    pub fn new(context: &IngestContext, name: impl Into<String>) -> Self {
        Self {
            common: CommonDoc::new(context, DocumentKind::Option, name),
            loc: Vec::new(),
            parents: Vec::new(),
            option_set: None,
            declarations: Vec::new(),
            description: None,
            option_type: None,
            default: None,
            example: None,
            related_packages: None,
            read_only: None,
            internal: None,
            visible: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum SearchDocument {
    Option(OptionDoc),
}

impl SearchDocument {
    pub fn common(&self) -> &CommonDoc {
        match self {
            Self::Option(doc) => &doc.common,
        }
    }

    pub fn id(&self) -> &str {
        &self.common().id
    }

    pub fn name(&self) -> &str {
        &self.common().name
    }

    pub fn kind(&self) -> &DocumentKind {
        &self.common().kind
    }
}

pub fn make_document_id(
    project: &str,
    dataset: &str,
    ref_id: &str,
    kind: &str,
    name: &str,
) -> String {
    format!(
        "{}/{}/{}/{}/{}",
        escape_id_part(project),
        escape_id_part(dataset),
        escape_id_part(ref_id),
        escape_id_part(kind),
        escape_id_part(name),
    )
}

fn escape_id_part(part: &str) -> String {
    part.replace('/', "%2F")
}

