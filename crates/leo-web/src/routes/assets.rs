use axum::http::header;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, MethodRouter};

macro_rules! theme {
    () => {
        concat!(
            include_str!("../web/styles/01-tokens.css"),
            "@media (prefers-color-scheme: dark) {\n  :root:not([data-theme=\"light\"]) {\n",
            include_str!("../web/dark-tokens.css"),
            "  }\n}\n:root[data-theme=\"dark\"] {\n",
            include_str!("../web/dark-tokens.css"),
            "  color-scheme: dark;\n}\n:root[data-theme=\"light\"] { color-scheme: light; }\n",
        )
    };
}

pub const THEME: &str = theme!();

const HTML: &str = concat!(
    include_str!("../web/page-head.html"),
    theme!(),
    include_str!("../web/styles/02-base.css"),
    include_str!("../web/styles/03-note.css"),
    include_str!("../web/styles/04-surfaces.css"),
    include_str!("../web/styles/05-doc-actions.css"),
    include_str!("../web/styles/06-felix.css"),
    include_str!("../web/styles/07-side-popup.css"),
    include_str!("../web/styles/08-chat.css"),
    include_str!("../web/styles/09-kanban.css"),
    include_str!("../web/styles/10-schedule.css"),
    include_str!("../web/styles/11-source-study.css"),
    include_str!("../web/styles/12-settings.css"),
    include_str!("../web/styles/13-setup-misc.css"),
    include_str!("../web/page-tail.html"),
);
pub(crate) const APP_JS: &str = concat!(
    "(function () {\n'use strict';\n",
    include_str!("../web/app/refresh.js"),
    include_str!("../web/app/core.js"),
    include_str!("../web/app/motion.js"),
    include_str!("../web/app/browse.js"),
    include_str!("../web/app/trash.js"),
    include_str!("../web/app/settings-navigation.js"),
    include_str!("../web/app/popup-menu.js"),
    include_str!("../web/app/select-menu.js"),
    include_str!("../web/app/settings.js"),
    include_str!("../web/app/dictation.js"),
    include_str!("../web/app/app-settings.js"),
    include_str!("../web/app/onboarding.js"),
    include_str!("../web/app/keeping.js"),
    include_str!("../web/app/uploads.js"),
    include_str!("../web/app/sheets.js"),
    include_str!("../web/app/side.js"),
    include_str!("../web/app/drag.js"),
    include_str!("../web/app/diagrams.js"),
    include_str!("../web/app/math.js"),
    include_str!("../web/app/sources.js"),
    include_str!("../web/app/passages.js"),
    include_str!("../web/app/margin-notes.js"),
    include_str!("../web/app/calendar.js"),
    include_str!("../web/app/boards.js"),
    include_str!("../web/app/actions.js"),
    include_str!("../web/app/beside.js"),
    include_str!("../web/app/events.js"),
    "})();\n",
);
pub(crate) const MARKDOWN_JS: &str = include_str!("../web/markdown.js");
const MERMAID_JS: &str = include_str!("../web/vendor/mermaid.min.js");
pub(crate) const MERMAID_PATH: &str = "/vendor/mermaid-11.17.2.js";
pub(crate) const EDITING_JS: &str = include_str!("../web/editing.js");
pub(crate) const SAVING_JS: &str = include_str!("../web/saving.js");
pub(crate) const DOC_JS: &str = include_str!("../web/doc.js");
pub(crate) const SCHEDULE_JS: &str = include_str!("../web/schedule.js");
pub(crate) const THEME_JS: &str = include_str!("../web/theme.js");
pub(crate) const CHAT_JS: &str = include_str!("../web/chat.js");
pub(crate) const PANE_RESIZE_JS: &str = include_str!("../web/pane-resize.js");
pub(crate) const MARGIN_JS: &str = include_str!("../web/margin.js");
pub(crate) const RECORDING_JS: &str = include_str!("../web/recording.js");

pub(crate) async fn index() -> Html<&'static str> {
    Html(HTML)
}

pub(crate) fn script<S: Clone + Send + Sync + 'static>(source: &'static str) -> MethodRouter<S> {
    get(move || async move {
        (
            [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
            source,
        )
            .into_response()
    })
}

const KATEX: &[(&str, &[u8], &str)] = &[
    (
        "katex.min.js",
        include_bytes!("../web/vendor/katex/katex.min.js"),
        "text/javascript; charset=utf-8",
    ),
    (
        "katex.min.css",
        include_bytes!("../web/vendor/katex/katex.min.css"),
        "text/css; charset=utf-8",
    ),
    (
        "fonts/KaTeX_AMS-Regular.woff2",
        include_bytes!("../web/vendor/katex/fonts/KaTeX_AMS-Regular.woff2"),
        "font/woff2",
    ),
    (
        "fonts/KaTeX_Caligraphic-Bold.woff2",
        include_bytes!("../web/vendor/katex/fonts/KaTeX_Caligraphic-Bold.woff2"),
        "font/woff2",
    ),
    (
        "fonts/KaTeX_Caligraphic-Regular.woff2",
        include_bytes!("../web/vendor/katex/fonts/KaTeX_Caligraphic-Regular.woff2"),
        "font/woff2",
    ),
    (
        "fonts/KaTeX_Fraktur-Bold.woff2",
        include_bytes!("../web/vendor/katex/fonts/KaTeX_Fraktur-Bold.woff2"),
        "font/woff2",
    ),
    (
        "fonts/KaTeX_Fraktur-Regular.woff2",
        include_bytes!("../web/vendor/katex/fonts/KaTeX_Fraktur-Regular.woff2"),
        "font/woff2",
    ),
    (
        "fonts/KaTeX_Main-Bold.woff2",
        include_bytes!("../web/vendor/katex/fonts/KaTeX_Main-Bold.woff2"),
        "font/woff2",
    ),
    (
        "fonts/KaTeX_Main-BoldItalic.woff2",
        include_bytes!("../web/vendor/katex/fonts/KaTeX_Main-BoldItalic.woff2"),
        "font/woff2",
    ),
    (
        "fonts/KaTeX_Main-Italic.woff2",
        include_bytes!("../web/vendor/katex/fonts/KaTeX_Main-Italic.woff2"),
        "font/woff2",
    ),
    (
        "fonts/KaTeX_Main-Regular.woff2",
        include_bytes!("../web/vendor/katex/fonts/KaTeX_Main-Regular.woff2"),
        "font/woff2",
    ),
    (
        "fonts/KaTeX_Math-BoldItalic.woff2",
        include_bytes!("../web/vendor/katex/fonts/KaTeX_Math-BoldItalic.woff2"),
        "font/woff2",
    ),
    (
        "fonts/KaTeX_Math-Italic.woff2",
        include_bytes!("../web/vendor/katex/fonts/KaTeX_Math-Italic.woff2"),
        "font/woff2",
    ),
    (
        "fonts/KaTeX_SansSerif-Bold.woff2",
        include_bytes!("../web/vendor/katex/fonts/KaTeX_SansSerif-Bold.woff2"),
        "font/woff2",
    ),
    (
        "fonts/KaTeX_SansSerif-Italic.woff2",
        include_bytes!("../web/vendor/katex/fonts/KaTeX_SansSerif-Italic.woff2"),
        "font/woff2",
    ),
    (
        "fonts/KaTeX_SansSerif-Regular.woff2",
        include_bytes!("../web/vendor/katex/fonts/KaTeX_SansSerif-Regular.woff2"),
        "font/woff2",
    ),
    (
        "fonts/KaTeX_Script-Regular.woff2",
        include_bytes!("../web/vendor/katex/fonts/KaTeX_Script-Regular.woff2"),
        "font/woff2",
    ),
    (
        "fonts/KaTeX_Size1-Regular.woff2",
        include_bytes!("../web/vendor/katex/fonts/KaTeX_Size1-Regular.woff2"),
        "font/woff2",
    ),
    (
        "fonts/KaTeX_Size2-Regular.woff2",
        include_bytes!("../web/vendor/katex/fonts/KaTeX_Size2-Regular.woff2"),
        "font/woff2",
    ),
    (
        "fonts/KaTeX_Size3-Regular.woff2",
        include_bytes!("../web/vendor/katex/fonts/KaTeX_Size3-Regular.woff2"),
        "font/woff2",
    ),
    (
        "fonts/KaTeX_Size4-Regular.woff2",
        include_bytes!("../web/vendor/katex/fonts/KaTeX_Size4-Regular.woff2"),
        "font/woff2",
    ),
    (
        "fonts/KaTeX_Typewriter-Regular.woff2",
        include_bytes!("../web/vendor/katex/fonts/KaTeX_Typewriter-Regular.woff2"),
        "font/woff2",
    ),
];

fn vendored(table: &'static [(&str, &[u8], &str)], path: &str) -> Response {
    match table.iter().find(|(name, _, _)| *name == path) {
        Some((_, bytes, kind)) => (
            [
                (header::CONTENT_TYPE, *kind),
                (
                    header::CACHE_CONTROL,
                    "private, max-age=31536000, immutable",
                ),
            ],
            *bytes,
        )
            .into_response(),
        None => axum::http::StatusCode::NOT_FOUND.into_response(),
    }
}

pub(crate) async fn katex_file(axum::extract::Path(path): axum::extract::Path<String>) -> Response {
    vendored(KATEX, &path)
}

const FONTS: &[(&str, &[u8], &str)] = &[
    (
        "instrument-sans-latin-wght-normal.woff2",
        include_bytes!("../web/vendor/fonts/instrument-sans-latin-wght-normal.woff2"),
        "font/woff2",
    ),
    (
        "instrument-sans-latin-wght-italic.woff2",
        include_bytes!("../web/vendor/fonts/instrument-sans-latin-wght-italic.woff2"),
        "font/woff2",
    ),
    (
        "newsreader-latin-opsz-normal.woff2",
        include_bytes!("../web/vendor/fonts/newsreader-latin-opsz-normal.woff2"),
        "font/woff2",
    ),
    (
        "newsreader-latin-opsz-italic.woff2",
        include_bytes!("../web/vendor/fonts/newsreader-latin-opsz-italic.woff2"),
        "font/woff2",
    ),
];

pub(crate) async fn font_file(axum::extract::Path(path): axum::extract::Path<String>) -> Response {
    vendored(FONTS, &path)
}

pub(crate) async fn mermaid_js() -> Response {
    (
        [
            (header::CONTENT_TYPE, "text/javascript; charset=utf-8"),
            (
                header::CACHE_CONTROL,
                "private, max-age=31536000, immutable",
            ),
        ],
        MERMAID_JS,
    )
        .into_response()
}

const FAVICON: &str = "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"-1 -1 46 34\" shape-rendering=\"crispEdges\">\
<rect x=\"0\" y=\"18\" width=\"5\" height=\"6\" fill=\"#b4cfe7\"/>\
<rect x=\"39\" y=\"18\" width=\"5\" height=\"6\" fill=\"#b4cfe7\"/>\
<rect x=\"5\" y=\"2\" width=\"34\" height=\"28\" fill=\"#b4cfe7\"/>\
<g shape-rendering=\"geometricPrecision\" fill=\"#19191b\">\
<rect x=\"11\" y=\"16\" width=\"4\" height=\"4\"/><rect x=\"21\" y=\"16\" width=\"4\" height=\"4\"/></g></svg>";

pub(crate) async fn favicon() -> Response {
    (
        [
            (header::CONTENT_TYPE, "image/svg+xml"),
            (header::CACHE_CONTROL, "public, max-age=86400"),
        ],
        FAVICON,
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_part_of_the_page_script_is_served() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/web/app");
        let mut parts = 0;
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|e| e == "js") {
                let text = std::fs::read_to_string(&path).unwrap();
                assert!(
                    APP_JS.contains(&text),
                    "{} is not in /app.js",
                    path.display()
                );
                parts += 1;
            }
        }
        assert!(parts > 0);
    }

    #[tokio::test]
    async fn the_bundled_fonts_are_served_immutable() {
        for (name, bytes, _) in FONTS {
            let response = font_file(axum::extract::Path(name.to_string())).await;
            assert_eq!(response.status(), axum::http::StatusCode::OK);
            assert_eq!(response.headers()[header::CONTENT_TYPE], "font/woff2");
            assert_eq!(
                response.headers()[header::CACHE_CONTROL],
                "private, max-age=31536000, immutable"
            );
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            assert_eq!(&body[..], *bytes);
            assert!(
                HTML.contains(&format!("/vendor/fonts-5.3.0/{name}")),
                "{name} is not in the page's @font-face rules"
            );
        }
        let missing = font_file(axum::extract::Path("nothing.woff2".into())).await;
        assert_eq!(missing.status(), axum::http::StatusCode::NOT_FOUND);
    }

    #[test]
    fn the_page_script_parses() {
        let file = tempfile::Builder::new().suffix(".js").tempfile().unwrap();
        std::fs::write(file.path(), APP_JS).unwrap();
        let Ok(out) = std::process::Command::new("node")
            .arg("--check")
            .arg(file.path())
            .output()
        else {
            eprintln!("skipping: node is not installed");
            return;
        };
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}
