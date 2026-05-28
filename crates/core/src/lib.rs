mod artifact;
mod document;
mod ingest;
mod name;
mod source_link;

pub use artifact::ArtifactKind;
pub use document::{
    CommonDoc, DocumentKind, License, Maintainer, OptionDoc, PackageDoc, SearchDocument,
};
pub use ingest::IngestContext;
pub use name::{NameParts, make_document_id};
pub use source_link::{Declaration, Repo, RepoHost, SourceLinkConfig, SourceLinkResolver};
