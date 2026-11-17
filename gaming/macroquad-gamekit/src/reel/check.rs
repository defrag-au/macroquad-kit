//! A reel's own checks: what a still's report and a recording's summary say beyond the kit's
//! text report.
//!
//! - **Missing glyphs.** Every string the reel sets, against its style's font. A font without a
//!   glyph draws nothing for it: the app's Noto subset dropped a ✓ and a ™ that way without a
//!   word. This runs before anything is drawn, and stops the run naming the line.
//! - **The safe area.** Text outside the margins a reel declares (`safe top=… bottom=…`), where
//!   a platform's own buttons and captions sit over the video. Reported for each still, and for a
//!   recording as one line per item with when it was outside.
//!
//! Pure: the font is asked through a closure, and boxes arrive measured.

use std::collections::BTreeMap;

use super::file::{Area, Look, Reel, Row, runs};

/// A string the reel sets in a style, and the line that sets it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Setting<'a> {
    pub line: usize,
    pub style: &'a str,
    pub text: String,
}

/// Every string the reel sets: text items, phone labels, a browser's address and chips, and
/// sheets' rows. Emphasis stars are markup, not text, so they are taken out.
pub fn settings(reel: &Reel) -> Vec<Setting<'_>> {
    let mut out = Vec::new();
    let mut set = |line: usize, style: &'_ str, text: &str| {
        let plain: String = runs(text).into_iter().map(|r| r.text).collect();
        out.push((line, style.to_string(), plain));
    };
    for shot in &reel.shots {
        for item in &shot.items {
            match &item.look {
                Look::Text { style, text, .. } => set(item.line, style, text),
                Look::Phone {
                    label: Some(label), ..
                } => set(item.line, &label.style, &label.text),
                Look::Browser { url, chips, .. } => {
                    for label in url.iter().chain(chips) {
                        set(item.line, &label.style, &label.text);
                    }
                }
                Look::Sheet { rows, .. } => {
                    for row in rows {
                        match row {
                            Row::Line { style, text } | Row::Boxed { style, text, .. } => {
                                set(item.line, style, text)
                            }
                            Row::Buttons { style, labels } => {
                                for l in labels {
                                    set(item.line, style, l);
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
    out.into_iter()
        .map(|(line, style, text)| Setting {
            line,
            style: reel
                .styles
                .get_key_value(&style)
                .map_or("", |(k, _)| k.as_str()),
            text,
        })
        .collect()
}

/// The characters each string's font has not got, as one message per line of the reel that
/// sets them. `has(font, c)` says whether the font named `font` draws `c`. Whitespace and the
/// zero-width space the layout keeps blank lines with are never asked about.
pub fn missing_glyphs(reel: &Reel, has: impl Fn(&str, char) -> bool) -> Vec<(usize, String)> {
    let mut found: BTreeMap<usize, (String, Vec<char>)> = BTreeMap::new();
    for s in settings(reel) {
        let Some(style) = reel.styles.get(s.style) else {
            continue;
        };
        for c in s.text.chars() {
            if c.is_whitespace() || c == '\u{200b}' || has(&style.font, c) {
                continue;
            }
            let entry = found
                .entry(s.line)
                .or_insert_with(|| (style.font.clone(), Vec::new()));
            if !entry.1.contains(&c) {
                entry.1.push(c);
            }
        }
    }
    found
        .into_iter()
        .map(|(line, (font, chars))| {
            let list: Vec<String> = chars
                .iter()
                .map(|c| format!("{c} (U+{:04X})", *c as u32))
                .collect();
            (
                line,
                format!(
                    "font `{font}` has no {}, which would draw as nothing",
                    list.join(", ")
                ),
            )
        })
        .collect()
}

/// A piece of text as drawn: the line that placed it, what it says, and its box in the frame.
#[derive(Clone, Debug, PartialEq)]
pub struct Drawn {
    pub line: usize,
    pub text: String,
    pub area: Area,
}

/// Whether `inner` lies within `outer`, give or take half a pixel.
pub fn within(outer: Area, inner: Area) -> bool {
    const SLACK: f32 = 0.5;
    inner.x >= outer.x - SLACK
        && inner.y >= outer.y - SLACK
        && inner.x + inner.w <= outer.x + outer.w + SLACK
        && inner.y + inner.h <= outer.y + outer.h + SLACK
}

/// The drawn text that is not within `safe`.
pub fn outside(safe: Area, drawn: &[Drawn]) -> Vec<&Drawn> {
    drawn.iter().filter(|d| !within(safe, d.area)).collect()
}

/// A recording's safe-area findings, gathered frame by frame: one entry per shot and line, with
/// each span of time its text was outside.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Outside {
    /// By shot and line.
    found: BTreeMap<(usize, usize), Stray>,
}

/// One item's text outside the safe area: what it says, and each span of seconds it was out.
type Stray = (String, Vec<[f32; 2]>);

impl Outside {
    /// Note this frame's text outside the safe area, in shot `shot` at `t` seconds, frames
    /// `step` seconds apart: a frame straight after the last one found continues its span.
    pub fn note(&mut self, shot: usize, t: f32, step: f32, drawn: &[&Drawn]) {
        for d in drawn {
            let (_, spans) = self
                .found
                .entry((shot, d.line))
                .or_insert_with(|| (d.text.clone(), Vec::new()));
            match spans.last_mut() {
                Some(span) if t - span[1] <= step * 1.5 => span[1] = t,
                _ => spans.push([t, t]),
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.found.is_empty()
    }

    /// One line per finding: `path:LINE: …`.
    pub fn lines(&self, reel: &Reel, path: &str) -> Vec<String> {
        self.found
            .iter()
            .map(|((shot, line), (text, spans))| {
                let name = reel.shots.get(*shot).map_or("?", |s| s.name.as_str());
                let when: Vec<String> = spans
                    .iter()
                    .map(|[from, to]| format!("{from:.2}s to {to:.2}s"))
                    .collect();
                format!(
                    "{path}:{line}: text outside the safe area in [{name}], {}: {:?}",
                    when.join(", "),
                    short(text)
                )
            })
            .collect()
    }
}

/// `text` on one line, cut at 48 characters.
pub fn short(text: &str) -> String {
    let one = text.replace('\n', " ");
    if one.chars().count() <= 48 {
        return one;
    }
    let cut: String = one.chars().take(47).collect();
    format!("{cut}…")
}

#[cfg(test)]
mod tests {
    use super::*;

    const REEL: &str = r##"
reel 1080x1920 30
font noto Noto.ttf
style cap noto 46 #ffffff
[a] 2
text cap at=540,100 "REGISTERED ✓"
text cap at=540,900 "*Fine* print"
phone @ph at=0,0 width=100 blank=9x16 label="It's ™" chip=cap
"##;

    #[test]
    fn a_glyph_the_font_has_not_got_is_named_with_its_line() {
        let reel = Reel::parse(REEL).unwrap();
        let ascii = |_: &str, c: char| c.is_ascii();
        let missing = missing_glyphs(&reel, ascii);
        assert_eq!(
            missing,
            vec![
                (
                    6,
                    "font `noto` has no ✓ (U+2713), which would draw as nothing".to_string()
                ),
                (
                    8,
                    "font `noto` has no ™ (U+2122), which would draw as nothing".to_string()
                ),
            ]
        );
    }

    #[test]
    fn emphasis_stars_are_not_text() {
        let reel = Reel::parse(REEL).unwrap();
        let texts: Vec<String> = settings(&reel).into_iter().map(|s| s.text).collect();
        assert!(texts.contains(&"Fine print".to_string()), "{texts:?}");
    }

    #[test]
    fn text_past_a_margin_is_outside_and_gathered_by_line() {
        let safe = Area {
            x: 60.0,
            y: 220.0,
            w: 900.0,
            h: 1320.0,
        };
        let at = |y: f32| Drawn {
            line: 7,
            text: "Two chains.\nTwo wallets.".into(),
            area: Area {
                x: 100.0,
                y,
                w: 800.0,
                h: 100.0,
            },
        };
        let drawn = [at(500.0), at(1500.0)];
        let out = outside(safe, &drawn);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].area.y, 1500.0);

        let reel = Reel::parse("reel 1080x1920 30\n[problem] 9").unwrap();
        let mut found = Outside::default();
        let step = 1.0 / 30.0;
        for f in 30..=45 {
            found.note(0, f as f32 * step, step, &out);
        }
        found.note(0, 2.5, step, &out);
        assert_eq!(
            found.lines(&reel, "promo.reel"),
            [
                "promo.reel:7: text outside the safe area in [problem], 1.00s to 1.50s, 2.50s to 2.50s: \"Two chains. Two wallets.\""
            ]
        );
    }
}
