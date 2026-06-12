use maud::{Markup, html};

use nixsearch_config::app::AppConfig;

use crate::entry::EntryData;
use crate::request::PageState;
use crate::urls::close_url_for_state;

use super::entry_article;

pub fn render(config: &AppConfig, page_state: &PageState, entry: &EntryData) -> Markup {
    match entry {
        EntryData::Empty => render_empty(),
        _ => render_modal(config, page_state, entry),
    }
}

fn render_empty() -> Markup {
    html! {
        div #entry-modal-container {}
    }
}

fn render_modal(config: &AppConfig, page_state: &PageState, entry: &EntryData) -> Markup {
    let close_href = close_url_for_state(config, page_state);

    html! {
        div #entry-modal-container {
            a.modal-backdrop href=(close_href) aria-label="Close modal" {}
            dialog #entry-modal data-close-url=(close_href) {
                (entry_article::render(config, page_state, entry, Some(&close_href)))
            }
        }
    }
}
