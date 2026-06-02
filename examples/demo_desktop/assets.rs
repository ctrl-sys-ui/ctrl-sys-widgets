/// Returns the embedded static asset bytes and its content-type for the given
/// path (relative to `/static/`, no leading slash), or `None` for unknown paths.
pub fn get_asset(path: &str) -> Option<(&'static [u8], &'static str)> {
    match path {
        "htmx.min.js" => Some((
            include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/static/htmx.min.js")),
            "application/javascript",
        )),
        "style.css" => Some((
            include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/static/style.css")),
            "text/css",
        )),
        "tooltip.js" => Some((
            include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/static/tooltip.js")),
            "application/javascript",
        )),
        "desktop_transport.js" => Some((
            include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/static/desktop_transport.js")),
            "application/javascript",
        )),
        "fonts/ibm-plex-mono/IBMPlexMono-Bold.woff2" => Some((
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/static/fonts/ibm-plex-mono/IBMPlexMono-Bold.woff2"
            )),
            "font/woff2",
        )),
        "fonts/ibm-plex-mono/IBMPlexMono-Medium.woff2" => Some((
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/static/fonts/ibm-plex-mono/IBMPlexMono-Medium.woff2"
            )),
            "font/woff2",
        )),
        "fonts/ibm-plex-mono/IBMPlexMono-Regular.woff2" => Some((
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/static/fonts/ibm-plex-mono/IBMPlexMono-Regular.woff2"
            )),
            "font/woff2",
        )),
        "fonts/inter/Inter-Bold.woff2" => Some((
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/static/fonts/inter/Inter-Bold.woff2"
            )),
            "font/woff2",
        )),
        "fonts/inter/Inter-Medium.woff2" => Some((
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/static/fonts/inter/Inter-Medium.woff2"
            )),
            "font/woff2",
        )),
        "fonts/inter/Inter-Regular.woff2" => Some((
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/static/fonts/inter/Inter-Regular.woff2"
            )),
            "font/woff2",
        )),
        _ => None,
    }
}
