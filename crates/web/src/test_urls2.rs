#[cfg(test)]
mod tests {
    use crate::request::*;
    use crate::urls::*;
    use nixsearch_config::app::AppConfig;
    use nixsearch_config::source::{RefConfig, SourceConfig, SourceKind};
    use nixsearch_core::document::*;
    use std::collections::BTreeMap;
    
    #[test]
    fn test_entry_url_for_document_cross_channel() {
        let mut config = AppConfig::default();
        config.sources.insert("nixpkgs".to_owned(), SourceConfig {
            kind: SourceKind::Nixpkgs,
            default_ref: Some("nixos-unstable".to_owned()),
            refs: vec![
                RefConfig { id: "nixos-unstable".to_owned(), ..Default::default() },
                RefConfig { id: "nixos-25.11".to_owned(), ..Default::default() },
            ],
            ..Default::default()
        });
        config.ref_sets.insert("unstable".to_owned(), nixsearch_config::app::RefSet {
            refs: {
                let mut map = BTreeMap::new();
                map.insert("nixpkgs".to_owned(), vec!["nixos-unstable".to_owned()]);
                map
            },
            ..Default::default()
        });
        
        let state = PageState {
            q: None,
            page: None,
            source_filter: SourceFilter::Named("nixpkgs".to_owned()),
            ref_scope: RefScope::RefSet { ref_set: "unstable".to_owned() },
            source_ref: None,
            detail: None,
        };
        
        let mut document = SearchDocument::NixpkgsOption(NixpkgsOptionDocument::default());
        document.common_mut().source = "nixpkgs".to_owned();
        document.common_mut().name = "foo".to_owned();
        document.common_mut().ref_id = "nixos-25.11".to_owned();
        
        let url = entry_url_for_document(&config, &state, &document, None);
        println!("URL: {}", url);
        assert!(!url.contains("ref_set="));
    }
}
