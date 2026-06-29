use axum::http::{HeaderName, HeaderValue};
use axum::response::Response;

pub(crate) const X_ROBOTS_TAG_NOINDEX_NOFOLLOW: &str = "noindex, nofollow";
const X_ROBOTS_TAG_HEADER: HeaderName = HeaderName::from_static("x-robots-tag");

pub(crate) async fn add_noindex_header(mut response: Response) -> Response {
    response.headers_mut().insert(
        X_ROBOTS_TAG_HEADER,
        HeaderValue::from_static(X_ROBOTS_TAG_NOINDEX_NOFOLLOW),
    );
    response
}
