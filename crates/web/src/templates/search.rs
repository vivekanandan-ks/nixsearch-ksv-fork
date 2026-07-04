use maud::{Markup, html};

use nixsearch_config::app::AppConfig;

use crate::request::{PageState, SourceFilter};
use crate::urls::{all_tab_url_from_state, source_tab_url_from_state};

use super::source_tag::color_for_source;


pub fn render_form(config: &AppConfig, state: &PageState, form_action: &str, q: &str) -> Markup {
    let has_multiple_sources = config
        .sources
        .values()
        .filter(|source| source.has_searchable_refs())
        .count()
        > 1;
    let source_color = match &state.source_filter {
        SourceFilter::Named(source_id) if config.sources.contains_key(source_id) => {
            Some(color_for_source(config, source_id))
        }
        SourceFilter::All => None,
        SourceFilter::Named(_) => None,
    };
    html! {
        form.search-form action=(form_action) method="get"
            style=[source_color.as_ref().map(|color| format!("--search-focus-color: {color};"))] {
            @if has_multiple_sources {
                (render_source_checkboxes(config, state))
            }

            div.search-bar-row {
                input type="search" name="q" value=(q)
                    placeholder="Search packages and options…"
                    autocomplete="off" autocorrect="off" autocapitalize="none" spellcheck="false" autofocus
                    data-nixsearch-input="q";

                @if q.is_empty() {
                    div.ref-radios.js-ref-radios
                        data-nixsearch-ref-container=""
                        style=[source_color.as_ref().map(|color| format!("--source-color: {color};"))] {
                        (render_ref_radios(config, state, false))
                    }

                    noscript {
                        (render_ref_links(config, state, source_color.as_deref()))
                    }
                } @else {
                    div style="display: none;" {
                        (render_ref_radios(config, state, true))
                    }
                }


            }

            button.search-submit type="submit" { "Search" }
        }
    }
}

fn render_source_checkboxes(config: &AppConfig, state: &PageState) -> Markup {
    let searchable_sources: Vec<_> = config
        .sources
        .iter()
        .filter(|(_, source)| source.has_searchable_refs())
        .collect();
        
    let all_selected = searchable_sources.iter().all(|(id, _)| {
        state.active_sources.contains(*id)
            || (matches!(&state.source_filter, SourceFilter::Named(s) if s == *id))
            || matches!(&state.source_filter, SourceFilter::All)
    });

    html! {
        div.source-checkboxes-container {
            @for (id, source) in searchable_sources {
                @let name = source.name.as_deref().unwrap_or(id);
                @let is_selected = state.active_sources.contains(id) 
                    || (matches!(&state.source_filter, SourceFilter::Named(s) if s == id));
                @let color = color_for_source(config, id);
                label.source-checkbox style=(format!("--checkbox-color: {color};")) {
                    input type="checkbox" name="source" value=(id) checked[is_selected] data-nixsearch-source-checkbox="";
                    span { (name) }
                    span.source-checkbox-close { "×" }
                }
            }
        }
    }
}

fn render_ref_links(config: &AppConfig, state: &PageState, source_color: Option<&str>) -> Markup {
    let single_source_id = match &state.source_filter {
        SourceFilter::Named(source_id) => Some(source_id.as_str()),
        SourceFilter::All if state.active_sources.len() == 1 => Some(state.active_sources[0].as_str()),
        _ => None,
    };

    if single_source_id.is_none() {
        return html! {
            div.ref-radios.noscript-ref-radios {
                @for ref_set in config.ref_sets.keys() {
                    @let is_selected = state.active_ref_set() == Some(ref_set.as_str());
                    @let next = {
                        let mut next = state.clone();
                        next.set_explicit_ref_set(ref_set.clone());
                        next
                    };
                    a.ref-radio-label.ref-radio-link href=(all_tab_url_from_state(config, &next))
                        data-active[is_selected] {
                        span.ref-radio-dot {}
                        span { (ref_set) }
                    }
                }
            }
        };
    }

    let source_id = single_source_id.unwrap();
    let Some(source) = config.sources.get(source_id) else {
        return html! {};
    };

    html! {
        div.ref-radios.noscript-ref-radios
            style=[source_color.map(|color| format!("--source-color: {color};"))] {
            @for ref_config in source.searchable_refs() {
                @let ref_id = ref_config.id.as_str();
                @let is_selected = state.source_ref.as_deref() == Some(ref_id);
                @let next = {
                    let mut next = state.clone();
                    next.source_ref.replace(ref_id.to_owned());
                    next.clear_ref_set_context();
                    next
                };
                a.ref-radio-label.ref-radio-link href=(source_tab_url_from_state(config, &next, source_id))
                    data-active[is_selected] {
                    span.ref-radio-dot {}
                    span { (ref_id) }
                }
            }
        }
    }
}

fn render_ref_radios(config: &AppConfig, state: &PageState, is_hidden: bool) -> Markup {
    let single_source_id = match &state.source_filter {
        SourceFilter::Named(source_id) => Some(source_id.as_str()),
        SourceFilter::All if state.active_sources.len() == 1 => Some(state.active_sources[0].as_str()),
        _ => None,
    };

    let (refs, current, input_name): (Vec<&str>, Option<&str>, &str) = match single_source_id {
        None => (
            config.ref_sets.keys().map(String::as_str).collect(),
            state.active_ref_set(),
            "ref_set",
        ),
        Some(source_id) => match config.sources.get(source_id) {
            Some(source) => (
                source.searchable_refs().map(|r| r.id.as_str()).collect(),
                state.source_ref.as_deref(),
                "ref",
            ),
            None => (Vec::new(), None, "ref"),
        },
    };

    html! {
        @for ref_id in &refs {
            @let is_selected = current == Some(*ref_id);
            @if is_hidden {
                @if is_selected {
                    input type="hidden" name=(input_name) value=(ref_id) data-nixsearch-input="ref";
                }
            } @else {
                label.ref-radio-label {
                    input type="radio" name=(input_name) value=(ref_id) checked[is_selected] data-nixsearch-input="ref";
                    span { (ref_id) }
                }
            }
        }
    }
}
