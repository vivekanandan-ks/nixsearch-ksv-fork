use maud::{Markup, html};

use nixsearch_config::app::AppConfig;
use nixsearch_core::document::SearchDocument;

use crate::entry::{AnnotatedEntryDocument, EntryData};
use crate::request::PageState;
use crate::urls::{entry_url_for_annotated_document, source_url_from_state};

use super::{detail, source_tag};

pub fn render(
    config: &AppConfig,
    page_state: &PageState,
    entry: &EntryData,
    close_href: Option<&str>,
) -> Markup {
    match entry {
        EntryData::Empty => html! {},
        EntryData::Found(entry) => render_found(config, page_state, &entry.document, close_href),
        EntryData::NotFound { entry } => render_not_found(entry, close_href),
        EntryData::Ambiguous(entries) => render_ambiguous(config, page_state, entries, close_href),
        EntryData::Error(error) => render_error(error, close_href),
    }
}

fn render_found(
    config: &AppConfig,
    page_state: &PageState,
    document: &SearchDocument,
    close_href: Option<&str>,
) -> Markup {
    let common = document.common();
    let source_color = source_tag::color_for_source(config, &common.source);
    let source_href = source_url_from_state(config, page_state, &common.source);

    html! {
        article.entry {
            header {
                div {
                    div.entry-title-row {
                        h1 { (common.name) }
                        button.entry-copy
                            type="button"
                            data-copy-entry=(common.name)
                            aria-label="Copy entry name"
                            title="Copy"
                            style=(format!("--source-color: {source_color};")) {
                            svg.copy-icon xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" aria-hidden="true" {
                                path d="M8 7a3 3 0 0 1 3-3h6a3 3 0 0 1 3 3v6a3 3 0 0 1-3 3h-1v1a3 3 0 0 1-3 3H7a3 3 0 0 1-3-3v-6a3 3 0 0 1 3-3h1V7Zm2 1h3a3 3 0 0 1 3 3v3h1a1 1 0 0 0 1-1V7a1 1 0 0 0-1-1h-6a1 1 0 0 0-1 1v1Zm-3 2a1 1 0 0 0-1 1v6a1 1 0 0 0 1 1h6a1 1 0 0 0 1-1v-6a1 1 0 0 0-1-1H7Z" {}
                            }
                            svg.check-icon xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" aria-hidden="true" {
                                path d="M9.55 17.6 4.7 12.75l1.4-1.4 3.45 3.45 8.35-8.35 1.4 1.4-9.75 9.75Z" {}
                            }
                        }
                    }
                    div.meta {
                        (source_tag::render_link(config, &common.source, &source_href))
                        (common.kind.as_str()) " · " (common.ref_id)
                        @if let Some(revision) = &common.revision {
                            " · " code { (&revision[..revision.len().min(8)]) }
                        }
                    }
                }
                (render_close_link(close_href))
            }
            (detail::render(document, config))
        }
    }
}

fn render_error(message: &str, close_href: Option<&str>) -> Markup {
    html! {
        article.entry {
            header {
                h1 { "Error" }
                (render_close_link(close_href))
            }
            div.results-error { (message) }
        }
    }
}

fn render_not_found(entry: &str, close_href: Option<&str>) -> Markup {
    html! {
        article.entry {
            header {
                h1 { "Entry not found" }
                (render_close_link(close_href))
            }
            div.results-error {
                "Entry " code { (entry) } " was not found."
            }
        }
    }
}

fn render_ambiguous(
    config: &AppConfig,
    page_state: &PageState,
    entries: &[AnnotatedEntryDocument],
    close_href: Option<&str>,
) -> Markup {
    html! {
        article.entry {
            header {
                h1 { "Multiple entries found" }
                (render_close_link(close_href))
            }
            p { "This name exists in multiple forms. Choose one:" }
            ul {
                @for entry in entries {
                    @let common = entry.document.common();
                    @let href = entry_url_for_annotated_document(config, page_state, entry, page_state.page);
                    li {
                        a href=(href) {
                            (common.kind.as_str()) " · " (common.source) "/" (common.ref_id)
                        }
                    }
                }
            }
        }
    }
}

fn render_close_link(close_href: Option<&str>) -> Markup {
    html! {
        @if let Some(close_href) = close_href {
            a.entry-close href=(close_href) data-role="entry-close" autofocus { "✕ Close" }
        }
    }
}
