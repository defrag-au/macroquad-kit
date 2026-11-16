//! The bundle's `index.html`, rendered from a shell template, and its web
//! app manifest.

use serde::{Deserialize, Serialize};

/// The shared shell: WebGL pre-flight, boot panel, panic mirroring.
pub const SHELL: &str = include_str!("../shell.html");

const TOKENS: [&str; 3] = ["__TITLE__", "__SCRIPTS__", "__WASM__"];

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Drop an authoring comment that sits before `<html`: it names the tokens,
/// and would otherwise have them substituted into their own documentation.
fn strip_authoring_comment(template: &str) -> String {
    let (Some(open), Some(html)) = (template.find("<!--"), template.find("<html")) else {
        return template.to_owned();
    };
    if open > html {
        return template.to_owned();
    }
    match template[open..].find("-->") {
        Some(len) => {
            let after = open + len + 3;
            let rest = template[after..].trim_start();
            format!("{}{rest}", &template[..open])
        }
        None => template.to_owned(),
    }
}

/// What a link to the page looks like where it is shared: the Open Graph and
/// Twitter card tags a chat app or a social site reads to draw a preview. A
/// canvas page has nothing else for them to read, so without these a shared
/// link is a bare URL.
///
/// Declared by the client crate itself, so every build of it carries the same
/// card with no flag to remember:
///
/// ```toml
/// [package.metadata.mq-bundle]
/// url = "https://robinhood.perp.quest/"
/// description = "Check your free-mint allocation before the snapshot."
/// image = "assets/card.jpg"          # in the bundle, or an absolute https URL
/// image-size = [1200, 630]
/// image-alt = "Cloning pods in Dr. Vape's lab"
/// ```
///
/// `url` is the page's public address: a preview's image must be an absolute
/// URL, so a bundle cannot work it out from where it happens to be served.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Card {
    /// The page's public address, `https://…`.
    pub url: String,
    /// One or two sentences under the title.
    pub description: String,
    /// The card's title, when it is not the page's own.
    #[serde(default)]
    pub title: Option<String>,
    /// The site the page belongs to, shown small above the title by some readers.
    #[serde(default)]
    pub site_name: Option<String>,
    /// The preview image: a path inside the bundle (relative to `url`), or an
    /// absolute `https://` URL. 1200×630 is what every reader draws well.
    #[serde(default)]
    pub image: Option<String>,
    /// The image's pixel size, so a reader lays the card out before fetching it.
    #[serde(default)]
    pub image_size: Option<(u32, u32)>,
    /// What the image shows, for a reader that does not draw it.
    #[serde(default)]
    pub image_alt: Option<String>,
}

impl Card {
    /// Refuse a card a reader could not use: an address that is not `https://`.
    pub fn check(&self) -> Result<(), String> {
        let https = |s: &str| s.starts_with("https://") && s.len() > "https://".len();
        if !https(&self.url) {
            return Err(format!(
                "[package.metadata.mq-bundle] url `{}` is not an https:// address; a preview's \
                 image and link must be absolute",
                self.url
            ));
        }
        if self.description.trim().is_empty() {
            return Err("[package.metadata.mq-bundle] description is empty".into());
        }
        Ok(())
    }

    /// The image inside the bundle, as a path relative to the bundle's root:
    /// `None` when there is no image or it is hosted elsewhere.
    pub fn bundled_image(&self) -> Option<&str> {
        self.image
            .as_deref()
            .filter(|i| !i.contains("://"))
            .map(|i| i.trim_start_matches('/'))
    }

    /// The image's absolute URL.
    fn image_url(&self) -> Option<String> {
        let image = self.image.as_deref()?;
        Some(match image.contains("://") {
            true => image.to_owned(),
            false => format!(
                "{}/{}",
                self.url.trim_end_matches('/'),
                image.trim_start_matches('/')
            ),
        })
    }

    /// The `<head>` tags, one a line. `page_title` is the page's own title, the
    /// card's unless it names another.
    pub fn head(&self, page_title: &str) -> String {
        let title = self.title.as_deref().unwrap_or(page_title);
        let mut tags: Vec<String> = Vec::new();
        let mut meta = |attr: &str, key: &str, content: &str| {
            tags.push(format!(
                "  <meta {attr}=\"{key}\" content=\"{}\">",
                escape(content)
            ));
        };
        meta("name", "description", &self.description);
        meta("property", "og:type", "website");
        meta("property", "og:title", title);
        meta("property", "og:description", &self.description);
        meta("property", "og:url", &self.url);
        if let Some(site) = &self.site_name {
            meta("property", "og:site_name", site);
        }
        let image = self.image_url();
        if let Some(image) = &image {
            meta("property", "og:image", image);
            if let Some((w, h)) = self.image_size {
                meta("property", "og:image:width", &w.to_string());
                meta("property", "og:image:height", &h.to_string());
            }
            if let Some(alt) = &self.image_alt {
                meta("property", "og:image:alt", alt);
            }
        }
        // The large card needs an image; without one, the small summary.
        meta(
            "name",
            "twitter:card",
            match image {
                Some(_) => "summary_large_image",
                None => "summary",
            },
        );
        meta("name", "twitter:title", title);
        meta("name", "twitter:description", &self.description);
        if let Some(image) = &image {
            meta("name", "twitter:image", image);
            if let Some(alt) = &self.image_alt {
                meta("name", "twitter:image:alt", alt);
            }
        }
        tags.push(format!(
            "  <link rel=\"canonical\" href=\"{}\">",
            escape(&self.url)
        ));
        tags.join("\n")
    }
}

/// The bundle's web app manifest, beside `index.html`.
pub const MANIFEST: &str = "manifest.webmanifest";

/// A web app manifest: what a phone needs to launch the bundle from its home
/// screen fullscreen, without the browser's bars.
#[derive(Serialize)]
pub struct WebManifest<'a> {
    pub name: &'a str,
    pub short_name: &'a str,
    pub display: &'static str,
    pub orientation: &'static str,
    pub background_color: &'static str,
    pub theme_color: &'static str,
    pub start_url: &'static str,
    pub scope: &'static str,
}

/// The manifest for a bundle titled `title`: fullscreen, any orientation, the
/// shell's background. URLs are relative to the manifest, so it works at any
/// base path.
pub fn manifest(title: &str) -> String {
    let m = WebManifest {
        name: title,
        short_name: title,
        display: "fullscreen",
        orientation: "any",
        background_color: "#050510",
        theme_color: "#050510",
        start_url: "./",
        scope: "./",
    };
    serde_json::to_string_pretty(&m).expect("a manifest of strings serialises")
}

/// Render `template`. `scripts` are the `src` values in load order, already
/// prefixed with any base path; `wasm` likewise, with its cache key; `base`
/// the base path itself, for the manifest link (`__MANIFEST__`, optional);
/// `card` the page's social card, written at `__HEAD__` (required of a template
/// only when there is a card to write).
pub fn render(
    template: &str,
    title: &str,
    scripts: &[String],
    wasm: &str,
    base: &str,
    card: Option<&Card>,
) -> Result<String, String> {
    let template = strip_authoring_comment(template);
    if let Some(missing) = TOKENS.iter().find(|t| !template.contains(**t)) {
        return Err(format!("shell template has no {missing} token"));
    }
    if card.is_some() && !template.contains("__HEAD__") {
        return Err(
            "the crate declares a social card ([package.metadata.mq-bundle]) but the shell \
             template has no __HEAD__ token to write it at"
                .into(),
        );
    }
    let head = card.map(|c| c.head(title)).unwrap_or_default();
    let tags = scripts
        .iter()
        .map(|src| format!("  <script src=\"{}\"></script>", escape(src)))
        .collect::<Vec<_>>()
        .join("\n");
    let manifest = format!(
        "  <link rel=\"manifest\" href=\"{}\">",
        escape(&format!("{base}{MANIFEST}"))
    );
    Ok(template
        .replace("__TITLE__", &escape(title))
        .replace("__SCRIPTS__", &tags)
        .replace("__MANIFEST__", &manifest)
        .replace("__HEAD__", &head)
        .replace("__WASM__", &escape(wasm)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shared_shell_renders_every_token() {
        let html = render(
            SHELL,
            "A & B",
            &["gl.js".into(), "/pump/x.js".into()],
            "g.wasm?v=1",
            "/pump/",
            None,
        )
        .expect("render");
        for t in TOKENS.iter().chain(&["__MANIFEST__", "__HEAD__"]) {
            assert!(!html.contains(t), "{t} left in the page");
        }
        assert!(html.contains("<link rel=\"manifest\" href=\"/pump/manifest.webmanifest\">"));
        assert!(html.contains("apple-mobile-web-app-capable"));
        assert!(html.contains("<title>A &amp; B</title>"));
        assert!(
            html.contains(
                "<script src=\"gl.js\"></script>\n  <script src=\"/pump/x.js\"></script>"
            )
        );
        assert!(html.contains("load(\"g.wasm?v=1\")"));
        assert!(
            html.starts_with("<!DOCTYPE html>\n<html"),
            "authoring comment kept"
        );
    }

    /// A page that dies says so and offers a way out, whatever killed it: the
    /// card, its Reload button and each route into it stay in the shell.
    #[test]
    fn the_shared_shell_turns_every_failure_into_a_reload_card() {
        for needle in [
            "window.__mq_fail = function",
            "button.textContent = \"Reload\"",
            "window.location.reload()",
            "role\", \"alert\"",
            "webglcontextlost",
            "visibilitychange",
            "addEventListener(\"unhandledrejection\"",
            "if (window.__mq_panic) return;",
            "<noscript>",
        ] {
            assert!(SHELL.contains(needle), "shell lost {needle:?}");
        }
        // Defined before the WebGL pre-flight, its first caller.
        let defined = SHELL.find("window.__mq_fail = function").expect("defined");
        let first_use = SHELL.find("window.__mq_fail(").expect("used");
        assert!(defined < first_use, "__mq_fail used before it is defined");
    }

    #[test]
    fn a_template_without_a_token_is_refused() {
        assert!(render("<html>__TITLE__ __WASM__</html>", "t", &[], "w", "", None).is_err());
    }

    #[test]
    fn a_custom_shell_need_not_link_a_manifest() {
        let html = render(
            "<html>__TITLE__ __SCRIPTS__ __WASM__</html>",
            "t",
            &[],
            "w",
            "",
            None,
        )
        .expect("renders without __MANIFEST__");
        assert!(!html.contains("manifest"));
    }

    fn card() -> Card {
        Card {
            url: "https://robinhood.perp.quest/".into(),
            description: "Check your \"free\" mints & more.".into(),
            title: None,
            site_name: Some("Perplings".into()),
            image: Some("assets/card.jpg".into()),
            image_size: Some((1200, 630)),
            image_alt: Some("Cloning pods".into()),
        }
    }

    #[test]
    fn a_card_is_written_into_the_shared_shells_head() {
        let html = render(SHELL, "Pre-Mint", &[], "g.wasm", "/", Some(&card())).expect("render");
        let head = &html[..html.find("</head>").expect("a head")];
        for needle in [
            "<meta property=\"og:title\" content=\"Pre-Mint\">",
            "<meta property=\"og:type\" content=\"website\">",
            "<meta property=\"og:url\" content=\"https://robinhood.perp.quest/\">",
            "<meta property=\"og:site_name\" content=\"Perplings\">",
            // Relative to the page's address, made absolute; escaped like any attribute.
            "<meta property=\"og:image\" content=\"https://robinhood.perp.quest/assets/card.jpg\">",
            "<meta property=\"og:image:width\" content=\"1200\">",
            "<meta property=\"og:image:height\" content=\"630\">",
            "<meta property=\"og:image:alt\" content=\"Cloning pods\">",
            "<meta name=\"description\" content=\"Check your &quot;free&quot; mints &amp; more.\">",
            "<meta name=\"twitter:card\" content=\"summary_large_image\">",
            "<meta name=\"twitter:image\" content=\"https://robinhood.perp.quest/assets/card.jpg\">",
            "<link rel=\"canonical\" href=\"https://robinhood.perp.quest/\">",
        ] {
            assert!(head.contains(needle), "head lacks {needle}");
        }
        assert!(!html.contains("__HEAD__"));
    }

    #[test]
    fn a_card_with_no_image_is_the_small_summary_and_keeps_its_own_title() {
        let c = Card {
            image: None,
            title: Some("The Pods".into()),
            ..card()
        };
        let head = c.head("Pre-Mint");
        assert!(head.contains("<meta name=\"twitter:card\" content=\"summary\">"));
        assert!(head.contains("<meta property=\"og:title\" content=\"The Pods\">"));
        assert!(!head.contains("og:image"));
        assert_eq!(c.bundled_image(), None);
    }

    #[test]
    fn an_image_hosted_elsewhere_is_used_as_given() {
        let c = Card {
            image: Some("https://cdn.example/card.png".into()),
            ..card()
        };
        assert!(
            c.head("t")
                .contains("content=\"https://cdn.example/card.png\"")
        );
        assert_eq!(c.bundled_image(), None);
        assert_eq!(card().bundled_image(), Some("assets/card.jpg"));
    }

    #[test]
    fn a_card_a_reader_could_not_use_is_refused() {
        assert!(card().check().is_ok());
        let http = Card {
            url: "http://robinhood.perp.quest/".into(),
            ..card()
        };
        assert!(http.check().unwrap_err().contains("https://"));
        let blank = Card {
            description: "  ".into(),
            ..card()
        };
        assert!(blank.check().is_err());
        // A shell with nowhere to write it says so, rather than dropping the card.
        let err = render(
            "<html>__TITLE__ __SCRIPTS__ __WASM__</html>",
            "t",
            &[],
            "w",
            "",
            Some(&card()),
        )
        .unwrap_err();
        assert!(err.contains("__HEAD__"), "{err}");
    }

    #[test]
    fn a_card_reads_from_the_crates_metadata_and_a_typo_is_loud() {
        let c: Card = serde_json::from_str(
            r#"{"url":"https://a.b/","description":"d","image":"assets/c.jpg",
                "image-size":[1200,630],"image-alt":"x","site-name":"S"}"#,
        )
        .expect("the documented keys parse");
        assert_eq!(c.image_size, Some((1200, 630)));
        assert_eq!(c.site_name.as_deref(), Some("S"));
        assert!(
            serde_json::from_str::<Card>(r#"{"url":"https://a.b/","description":"d","imgae":"x"}"#)
                .is_err()
        );
    }

    #[test]
    fn the_manifest_launches_fullscreen_in_any_orientation() {
        let m: serde_json::Value =
            serde_json::from_str(&manifest("Island \"Funk\"")).expect("json");
        assert_eq!(m["name"], "Island \"Funk\"");
        assert_eq!(m["display"], "fullscreen");
        assert_eq!(m["orientation"], "any");
        assert_eq!(m["start_url"], "./");
    }
}
