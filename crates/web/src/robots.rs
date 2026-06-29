use axum::http::{HeaderName, HeaderValue};
use axum::response::Response;

pub(crate) const ROBOTS_NOINDEX_FOLLOW: &str = "noindex,follow";
pub(crate) const X_ROBOTS_TAG_NOINDEX_NOFOLLOW: &str = "noindex, nofollow";

const ROBOTS_TXT_DISALLOW_ALL: &str = "User-agent: *\nDisallow: /\n";
const X_ROBOTS_TAG_HEADER: HeaderName = HeaderName::from_static("x-robots-tag");

pub(crate) async fn add_noindex_header(mut response: Response) -> Response {
    response.headers_mut().insert(
        X_ROBOTS_TAG_HEADER,
        HeaderValue::from_static(X_ROBOTS_TAG_NOINDEX_NOFOLLOW),
    );
    response
}

pub(crate) fn robots_txt_body(public_seo_enabled: bool, public_origin: Option<&str>) -> String {
    if !public_seo_enabled {
        return ROBOTS_TXT_DISALLOW_ALL.to_owned();
    }

    let public_origin = public_origin.expect("public SEO requires a public origin");
    format!("User-agent: *\nAllow: /\nSitemap: {public_origin}/sitemap.xml\n")
}
