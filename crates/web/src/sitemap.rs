const MAX_SITEMAP_URLS: usize = 50_000;
const MAX_SITEMAP_BYTES: usize = 50 * 1024 * 1024;
const MAX_SITEMAP_INDEX_ENTRIES: usize = 50_000;
const MAX_SITEMAP_INDEX_BYTES: usize = 50 * 1024 * 1024;
const SITEMAP_XML_NAMESPACE: &str = "http://www.sitemaps.org/schemas/sitemap/0.9";
const SITEMAP_SHARD_QUERY_PARAM: &str = "shard";
const SITEMAP_SHARD_WIDTH: usize = 5;
const SITEMAP_MAX_SHARD_NUMBER: usize = 99_999;
const SITEMAP_XML_DECLARATION: &str = r#"<?xml version="1.0" encoding="UTF-8"?>"#;
const SITEMAP_URLSET_CLOSE: &str = "</urlset>";
const SITEMAP_INDEX_CLOSE: &str = "</sitemapindex>";

#[derive(Debug, Clone, Copy)]
pub(crate) struct SitemapLimits {
    pub max_urlset_urls: usize,
    pub max_urlset_bytes: usize,
    pub max_index_entries: usize,
    pub max_index_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SitemapShard {
    number: usize,
    query_value: String,
    entries: Vec<String>,
    byte_len: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SitemapDocument {
    Urlset(String),
    Index(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SitemapQuery {
    EntryPoint,
    Shard(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SitemapQueryError {
    UnknownQuery,
    DuplicateShard,
    EmptyShard,
    MalformedShard,
    ShardOutOfRange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SitemapRenderError {
    MalformedQuery(SitemapQueryError),
    ShardNotAvailable,
    ShardOutOfRange,
    IndexTooLarge,
    NoRepresentableUrls,
}

pub(crate) fn protocol_sitemap_limits() -> SitemapLimits {
    SitemapLimits {
        max_urlset_urls: MAX_SITEMAP_URLS,
        max_urlset_bytes: MAX_SITEMAP_BYTES,
        max_index_entries: MAX_SITEMAP_INDEX_ENTRIES,
        max_index_bytes: MAX_SITEMAP_INDEX_BYTES,
    }
}

pub(crate) fn render_sitemap_entrypoint(
    origin: &str,
    mut paths: Vec<String>,
    raw_query: Option<&str>,
    limits: SitemapLimits,
) -> Result<SitemapDocument, SitemapRenderError> {
    let query = parse_sitemap_query(raw_query).map_err(SitemapRenderError::MalformedQuery)?;
    paths.sort();
    paths.dedup();

    let shards = shard_sitemap_paths(origin, paths, limits)?;
    let sitemap_index = if shards.len() > 1 {
        Some(render_sitemap_index(origin, &shards, limits)?)
    } else {
        None
    };

    match query {
        SitemapQuery::EntryPoint if shards.len() == 1 => Ok(SitemapDocument::Urlset(
            render_urlset_from_entries(&shards[0].entries),
        )),
        SitemapQuery::EntryPoint => Ok(SitemapDocument::Index(
            sitemap_index.expect("multi-shard sitemap index is rendered"),
        )),
        SitemapQuery::Shard(_) if shards.len() <= 1 => Err(SitemapRenderError::ShardNotAvailable),
        SitemapQuery::Shard(number) => shards
            .iter()
            .find(|shard| shard.number == number)
            .map(|shard| SitemapDocument::Urlset(render_urlset_from_entries(&shard.entries)))
            .ok_or(SitemapRenderError::ShardOutOfRange),
    }
}

fn parse_sitemap_query(raw_query: Option<&str>) -> Result<SitemapQuery, SitemapQueryError> {
    let Some(raw_query) = raw_query else {
        return Ok(SitemapQuery::EntryPoint);
    };

    let shard_prefix = format!("{SITEMAP_SHARD_QUERY_PARAM}=");
    let shard_count = raw_query
        .split('&')
        .filter(|part| part.starts_with(&shard_prefix))
        .count();

    if shard_count > 1 {
        return Err(SitemapQueryError::DuplicateShard);
    }

    if raw_query.contains('&') || !raw_query.starts_with(&shard_prefix) {
        return Err(SitemapQueryError::UnknownQuery);
    }

    let value = &raw_query[shard_prefix.len()..];
    let number = parse_sitemap_shard_query_value(value)?;

    Ok(SitemapQuery::Shard(number))
}

fn parse_sitemap_shard_query_value(value: &str) -> Result<usize, SitemapQueryError> {
    if value.is_empty() {
        return Err(SitemapQueryError::EmptyShard);
    }

    if value.len() != SITEMAP_SHARD_WIDTH || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(SitemapQueryError::MalformedShard);
    }

    let number = value
        .parse::<usize>()
        .map_err(|_| SitemapQueryError::MalformedShard)?;

    if number == 0 || number > SITEMAP_MAX_SHARD_NUMBER {
        return Err(SitemapQueryError::ShardOutOfRange);
    }

    Ok(number)
}

fn sitemap_shard_query_value(number: usize) -> Option<String> {
    (1..=SITEMAP_MAX_SHARD_NUMBER)
        .contains(&number)
        .then(|| format!("{number:0SITEMAP_SHARD_WIDTH$}"))
}

fn sitemap_shard_location_path_and_query(number: usize) -> Option<String> {
    sitemap_shard_query_value(number)
        .map(|value| format!("/sitemap.xml?{SITEMAP_SHARD_QUERY_PARAM}={value}"))
}

fn render_url_entry(origin: &str, path: &str) -> String {
    render_loc_entry("url", origin, path)
}

fn render_index_entry(origin: &str, shard_number: usize) -> Option<String> {
    let path = sitemap_shard_location_path_and_query(shard_number)?;

    Some(render_loc_entry("sitemap", origin, &path))
}

fn render_loc_entry(element: &str, origin: &str, path: &str) -> String {
    let url = format!("{origin}{path}");
    let loc = html_escape::encode_text(&url);

    format!("<{element}><loc>{loc}</loc></{element}>")
}

fn render_urlset_from_entries(entries: &[String]) -> String {
    format!(
        "{}{}{}",
        urlset_prefix(),
        entries.iter().map(String::as_str).collect::<String>(),
        SITEMAP_URLSET_CLOSE
    )
}

fn render_sitemap_index_from_entries(entries: &[String]) -> String {
    format!(
        "{}{}{}",
        sitemap_index_prefix(),
        entries.iter().map(String::as_str).collect::<String>(),
        SITEMAP_INDEX_CLOSE
    )
}

fn shard_sitemap_paths(
    origin: &str,
    paths: Vec<String>,
    limits: SitemapLimits,
) -> Result<Vec<SitemapShard>, SitemapRenderError> {
    if limits.max_urlset_urls == 0 || urlset_document_len(0) > limits.max_urlset_bytes {
        return Err(SitemapRenderError::NoRepresentableUrls);
    }

    let mut shards = Vec::new();
    let mut entries = Vec::new();
    let mut entries_len = 0;
    let mut skipped_entries = 0;

    for path in paths {
        let entry = render_url_entry(origin, &path);
        let entry_len = entry.len();

        if urlset_document_len(entry_len) > limits.max_urlset_bytes {
            skipped_entries += 1;
            tracing::warn!(
                path = %path,
                entry_bytes = entry_len,
                max_urlset_bytes = limits.max_urlset_bytes,
                "skipping sitemap URL entry that cannot fit in one sitemap document"
            );
            continue;
        }

        let next_count = entries.len() + 1;
        let next_len = urlset_document_len(entries_len + entry_len);
        if !entries.is_empty()
            && (next_count > limits.max_urlset_urls || next_len > limits.max_urlset_bytes)
        {
            push_sitemap_shard(&mut shards, &mut entries, &mut entries_len)?;
        }

        entries_len += entry_len;
        entries.push(entry);
    }

    if !entries.is_empty() {
        push_sitemap_shard(&mut shards, &mut entries, &mut entries_len)?;
    }

    if skipped_entries > 0 {
        tracing::warn!(
            skipped_entries,
            "skipped sitemap URL entries that exceeded sitemap document size limits"
        );
    }

    if shards.is_empty() {
        return Err(SitemapRenderError::NoRepresentableUrls);
    }

    Ok(shards)
}

fn push_sitemap_shard(
    shards: &mut Vec<SitemapShard>,
    entries: &mut Vec<String>,
    entries_len: &mut usize,
) -> Result<(), SitemapRenderError> {
    let number = shards.len() + 1;
    let Some(query_value) = sitemap_shard_query_value(number) else {
        return Err(SitemapRenderError::IndexTooLarge);
    };

    shards.push(SitemapShard {
        number,
        query_value,
        entries: std::mem::take(entries),
        byte_len: urlset_document_len(*entries_len),
    });
    *entries_len = 0;

    Ok(())
}

fn render_sitemap_index(
    origin: &str,
    shards: &[SitemapShard],
    limits: SitemapLimits,
) -> Result<String, SitemapRenderError> {
    if shards.len() > limits.max_index_entries || shards.len() > SITEMAP_MAX_SHARD_NUMBER {
        return Err(SitemapRenderError::IndexTooLarge);
    }

    if sitemap_index_document_len(0) > limits.max_index_bytes {
        return Err(SitemapRenderError::IndexTooLarge);
    }

    let entries = shards
        .iter()
        .map(|shard| {
            render_index_entry(origin, shard.number).ok_or(SitemapRenderError::IndexTooLarge)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let rendered = render_sitemap_index_from_entries(&entries);

    if rendered.len() > limits.max_index_bytes {
        return Err(SitemapRenderError::IndexTooLarge);
    }

    Ok(rendered)
}

fn urlset_prefix() -> String {
    format!(r#"{SITEMAP_XML_DECLARATION}<urlset xmlns="{SITEMAP_XML_NAMESPACE}">"#)
}

fn sitemap_index_prefix() -> String {
    format!(r#"{SITEMAP_XML_DECLARATION}<sitemapindex xmlns="{SITEMAP_XML_NAMESPACE}">"#)
}

fn urlset_document_len(entries_len: usize) -> usize {
    urlset_prefix().len() + entries_len + SITEMAP_URLSET_CLOSE.len()
}

fn sitemap_index_document_len(entries_len: usize) -> usize {
    sitemap_index_prefix().len() + entries_len + SITEMAP_INDEX_CLOSE.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORIGIN: &str = "https://search.example.com";

    fn limits(
        max_urlset_urls: usize,
        max_urlset_bytes: usize,
        max_index_entries: usize,
        max_index_bytes: usize,
    ) -> SitemapLimits {
        SitemapLimits {
            max_urlset_urls,
            max_urlset_bytes,
            max_index_entries,
            max_index_bytes,
        }
    }

    fn generous_limits() -> SitemapLimits {
        limits(50_000, 1_000_000, 50_000, 1_000_000)
    }

    fn paths(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    fn document_body(document: SitemapDocument) -> String {
        match document {
            SitemapDocument::Urlset(body) | SitemapDocument::Index(body) => body,
        }
    }

    #[test]
    fn parse_sitemap_query_accepts_entrypoint_and_canonical_shard() {
        assert_eq!(parse_sitemap_query(None), Ok(SitemapQuery::EntryPoint));
        assert_eq!(
            parse_sitemap_query(Some("shard=00001")),
            Ok(SitemapQuery::Shard(1))
        );
        assert_eq!(
            parse_sitemap_query(Some("shard=99999")),
            Ok(SitemapQuery::Shard(99_999))
        );
    }

    #[test]
    fn parse_sitemap_query_rejects_noncanonical_queries() {
        for raw_query in [
            "",
            "foo=bar",
            "shard=",
            "shard=1",
            "shard=001",
            "shard=00000",
            "shard=100000",
            "shard=abcde",
            "shard=%30%30%30%30%31",
            "shard=00001&x=1",
            "x=1&shard=00001",
            "shard=00001&shard=00002",
            "shard=00001/extra",
        ] {
            assert!(
                parse_sitemap_query(Some(raw_query)).is_err(),
                "accepted query {raw_query:?}"
            );
        }
    }

    #[test]
    fn sitemap_shard_query_values_are_canonical() {
        assert_eq!(sitemap_shard_query_value(1).as_deref(), Some("00001"));
        assert_eq!(sitemap_shard_query_value(99_999).as_deref(), Some("99999"));
        assert_eq!(sitemap_shard_query_value(0), None);
        assert_eq!(sitemap_shard_query_value(100_000), None);
        assert_eq!(
            sitemap_shard_location_path_and_query(1).as_deref(),
            Some("/sitemap.xml?shard=00001")
        );
    }

    #[test]
    fn shard_sitemap_paths_splits_by_url_count() {
        let shards = shard_sitemap_paths(
            ORIGIN,
            paths(&["/", "/a", "/b"]),
            limits(1, 1_000_000, 50_000, 1_000_000),
        )
        .unwrap();

        assert_eq!(shards.len(), 3);
        assert_eq!(shards[0].query_value, "00001");
        assert_eq!(shards[1].query_value, "00002");
        assert_eq!(shards[2].query_value, "00003");
    }

    #[test]
    fn shard_sitemap_paths_splits_by_full_document_byte_size() {
        let first = render_url_entry(ORIGIN, "/a");
        let second = render_url_entry(ORIGIN, "/b");
        let one_entry_len = render_urlset_from_entries(std::slice::from_ref(&first)).len();
        let two_entry_len = render_urlset_from_entries(&[first, second]).len();

        let shards = shard_sitemap_paths(
            ORIGIN,
            paths(&["/a", "/b"]),
            limits(50_000, two_entry_len - 1, 50_000, 1_000_000),
        )
        .unwrap();

        assert!(one_entry_len <= two_entry_len - 1);
        assert_eq!(shards.len(), 2);
        assert!(
            shards
                .iter()
                .all(|shard| shard.byte_len <= two_entry_len - 1)
        );
    }

    #[test]
    fn exact_byte_limit_fits() {
        let entry = render_url_entry(ORIGIN, "/a");
        let document_len = render_urlset_from_entries(std::slice::from_ref(&entry)).len();
        let document = render_sitemap_entrypoint(
            ORIGIN,
            paths(&["/a"]),
            None,
            limits(1, document_len, 50_000, 1_000_000),
        )
        .unwrap();

        assert_eq!(document_body(document).len(), document_len);
    }

    #[test]
    fn byte_accounting_matches_rendered_urlset_length() {
        let shards = shard_sitemap_paths(ORIGIN, paths(&["/a", "/b"]), generous_limits()).unwrap();
        let rendered = render_urlset_from_entries(&shards[0].entries);

        assert_eq!(shards[0].byte_len, rendered.len());
    }

    #[test]
    fn oversized_single_entry_is_skipped() {
        let short_entry = render_url_entry(ORIGIN, "/ok");
        let short_document_len =
            render_urlset_from_entries(std::slice::from_ref(&short_entry)).len();
        let shards = shard_sitemap_paths(
            ORIGIN,
            paths(&["/ok", "/this-path-is-too-long-to-fit"]),
            limits(50_000, short_document_len, 50_000, 1_000_000),
        )
        .unwrap();
        let rendered = render_urlset_from_entries(&shards[0].entries);

        assert!(rendered.contains("/ok"));
        assert!(!rendered.contains("this-path-is-too-long"));
    }

    #[test]
    fn all_skipped_input_returns_no_representable_urls() {
        assert_eq!(
            shard_sitemap_paths(
                ORIGIN,
                paths(&["/too-long"]),
                limits(50_000, urlset_document_len(0), 50_000, 1_000_000),
            ),
            Err(SitemapRenderError::NoRepresentableUrls)
        );
    }

    #[test]
    fn impossible_urlset_limits_return_no_representable_urls() {
        assert_eq!(
            shard_sitemap_paths(
                ORIGIN,
                paths(&["/"]),
                limits(0, 1_000_000, 50_000, 1_000_000)
            ),
            Err(SitemapRenderError::NoRepresentableUrls)
        );
        assert_eq!(
            shard_sitemap_paths(
                ORIGIN,
                paths(&["/"]),
                limits(50_000, urlset_document_len(0) - 1, 50_000, 1_000_000),
            ),
            Err(SitemapRenderError::NoRepresentableUrls)
        );
    }

    #[test]
    fn entrypoint_renders_urlset_for_single_shard() {
        let document =
            render_sitemap_entrypoint(ORIGIN, paths(&["/"]), None, generous_limits()).unwrap();
        let SitemapDocument::Urlset(body) = document else {
            panic!("expected urlset");
        };

        assert!(body.contains("<urlset"));
        assert!(body.contains("<loc>https://search.example.com/</loc>"));
        assert!(!body.contains("<sitemapindex"));
    }

    #[test]
    fn entrypoint_renders_sitemap_index_for_multiple_shards() {
        let document = render_sitemap_entrypoint(
            ORIGIN,
            paths(&["/", "/a"]),
            None,
            limits(1, 1_000_000, 50_000, 1_000_000),
        )
        .unwrap();
        let SitemapDocument::Index(body) = document else {
            panic!("expected sitemap index");
        };

        assert!(body.contains("<sitemapindex"));
        assert!(body.contains("<loc>https://search.example.com/sitemap.xml?shard=00001</loc>"));
        assert!(body.contains("<loc>https://search.example.com/sitemap.xml?shard=00002</loc>"));
    }

    #[test]
    fn shard_query_renders_selected_urlset_only_for_multiple_shards() {
        let document = render_sitemap_entrypoint(
            ORIGIN,
            paths(&["/", "/a"]),
            Some("shard=00002"),
            limits(1, 1_000_000, 50_000, 1_000_000),
        )
        .unwrap();
        let SitemapDocument::Urlset(body) = document else {
            panic!("expected urlset");
        };

        assert!(body.contains("<loc>https://search.example.com/a</loc>"));
        assert!(!body.contains("<loc>https://search.example.com/</loc>"));
    }

    #[test]
    fn shard_query_is_unavailable_for_single_shard() {
        assert_eq!(
            render_sitemap_entrypoint(
                ORIGIN,
                paths(&["/"]),
                Some("shard=00001"),
                generous_limits(),
            ),
            Err(SitemapRenderError::ShardNotAvailable)
        );
    }

    #[test]
    fn out_of_range_shard_returns_error() {
        assert_eq!(
            render_sitemap_entrypoint(
                ORIGIN,
                paths(&["/", "/a"]),
                Some("shard=00003"),
                limits(1, 1_000_000, 50_000, 1_000_000),
            ),
            Err(SitemapRenderError::ShardOutOfRange)
        );
    }

    #[test]
    fn malformed_query_preserves_query_error() {
        assert_eq!(
            render_sitemap_entrypoint(ORIGIN, paths(&["/"]), Some("foo=bar"), generous_limits()),
            Err(SitemapRenderError::MalformedQuery(
                SitemapQueryError::UnknownQuery
            ))
        );
    }

    #[test]
    fn sitemap_index_count_overflow_returns_error() {
        assert_eq!(
            render_sitemap_entrypoint(
                ORIGIN,
                paths(&["/", "/a"]),
                None,
                limits(1, 1_000_000, 1, 1_000_000),
            ),
            Err(SitemapRenderError::IndexTooLarge)
        );
    }

    #[test]
    fn shard_query_also_enforces_sitemap_index_limits() {
        assert_eq!(
            render_sitemap_entrypoint(
                ORIGIN,
                paths(&["/", "/a"]),
                Some("shard=00001"),
                limits(1, 1_000_000, 1, 1_000_000),
            ),
            Err(SitemapRenderError::IndexTooLarge)
        );
    }

    #[test]
    fn sitemap_index_byte_overflow_returns_error() {
        assert_eq!(
            render_sitemap_entrypoint(
                ORIGIN,
                paths(&["/", "/a"]),
                None,
                limits(1, 1_000_000, 50_000, sitemap_index_document_len(0)),
            ),
            Err(SitemapRenderError::IndexTooLarge)
        );
    }

    #[test]
    fn xml_escapes_url_and_index_locations() {
        let url_entry =
            render_url_entry("https://example.com&x=<tag>", "/fixtures/git?kind=package");
        let index_entry = render_index_entry("https://example.com&x=<tag>", 1).unwrap();

        assert!(
            url_entry.contains("https://example.com&amp;x=&lt;tag&gt;/fixtures/git?kind=package")
        );
        assert!(
            index_entry.contains("https://example.com&amp;x=&lt;tag&gt;/sitemap.xml?shard=00001")
        );
        assert!(!url_entry.contains("https://example.com&x=<tag>"));
        assert!(!index_entry.contains("https://example.com&x=<tag>"));
    }

    #[test]
    fn entrypoint_sorts_and_deduplicates_paths() {
        let document =
            render_sitemap_entrypoint(ORIGIN, paths(&["/b", "/a", "/a"]), None, generous_limits())
                .unwrap();
        let body = document_body(document);
        let a_index = body.find("/a</loc>").unwrap();
        let b_index = body.find("/b</loc>").unwrap();

        assert!(a_index < b_index);
        assert_eq!(body.matches("/a</loc>").count(), 1);
    }
}
