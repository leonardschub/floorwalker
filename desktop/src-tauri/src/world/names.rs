//! What the world writes, pure and free of Bevy (spec: "Robots and faces", "Assets",
//! Text): which company a model belongs to and the face that shows it, and how a name
//! is fitted to its plate: characters the font lacks become "?", a sign shrinks a long
//! name down to a minimum size, and what still does not fit is cut with "…".
//! `lettering` draws the results.

use skrifa::instance::{Location, Size};
use skrifa::metrics::GlyphMetrics;
use skrifa::{FontRef, MetadataProvider};

/// A model's company, told by its id's prefix only, never by its display name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Company {
    Anthropic,
    OpenAi,
    XAi,
    Google,
    /// Any other model, or none yet.
    Unknown,
}

impl Company {
    /// The company of model `id`:
    ///
    /// - `claude-…` is Anthropic;
    /// - `gpt-…` is OpenAI, and so is its o-series: `o`, one or more digits, then the
    ///   end or `-…` (`o3`, `o4-mini`, `o1-preview`), but not `omni-…`, `o3x` or `o`;
    /// - `grok-…` is xAI;
    /// - `gemini-…` is Google;
    /// - anything else, or no id, is unknown.
    ///
    /// Matched as written: ids are lower case.
    pub fn of(id: Option<&str>) -> Self {
        let Some(id) = id else {
            return Company::Unknown;
        };
        let o_series = id.strip_prefix('o').is_some_and(|rest| {
            let version = rest.split_once('-').map_or(rest, |(version, _)| version);
            !version.is_empty() && version.bytes().all(|b| b.is_ascii_digit())
        });
        if id.starts_with("claude-") {
            Company::Anthropic
        } else if id.starts_with("gpt-") || o_series {
            Company::OpenAi
        } else if id.starts_with("grok-") {
            Company::XAi
        } else if id.starts_with("gemini-") {
            Company::Google
        } else {
            Company::Unknown
        }
    }

    /// The face's colour, sRGB: Anthropic terracotta, OpenAI white, xAI silver, Google
    /// blue, and a neutral grey, darker than the silver, for an unknown model.
    pub fn colour(self) -> [f32; 3] {
        match self {
            Company::Anthropic => [0.85, 0.42, 0.27],
            Company::OpenAi => [0.96, 0.96, 0.94],
            Company::XAi => [0.66, 0.70, 0.76],
            Company::Google => [0.20, 0.47, 0.98],
            Company::Unknown => [0.38, 0.38, 0.38],
        }
    }
}

/// What a robot's face shows: its company's pattern, and the model's name beneath it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Face {
    pub company: Company,
    /// The model's display name (an unknown model's is its id); none before the agent
    /// has answered.
    pub name: Option<String>,
}

impl Face {
    /// The face of an agent whose model has `id` and display name `name` (each none
    /// before the agent has answered): no model gives the grey pattern and no name.
    pub fn of(id: Option<&str>, name: Option<&str>) -> Self {
        Face {
            company: Company::of(id),
            name: id.map(|id| name.unwrap_or(id).to_string()),
        }
    }
}

/// The mark that ends a cut name.
pub const ELLIPSIS: char = '…';

/// `text` on one line with every character the font lacks (`has` says no, control
/// characters included) shown as "?".
pub fn known(text: &str, has: impl Fn(char) -> bool) -> String {
    text.trim()
        .chars()
        .map(|c| if !c.is_control() && has(c) { c } else { '?' })
        .collect()
}

/// `text` as it fits `max` wide, `width` measuring: whole if it fits, else its longest
/// start that fits with "…" after it, else "…" alone.
pub fn cut(text: &str, max: f32, width: impl Fn(&str) -> f32) -> String {
    if width(text) <= max {
        return text.to_string();
    }
    let ends: Vec<usize> = text.char_indices().map(|(at, _)| at).collect();
    for &end in ends.iter().rev() {
        let cut = format!("{}{ELLIPSIS}", text[..end].trim_end());
        if width(&cut) <= max {
            return cut;
        }
    }
    ELLIPSIS.to_string()
}

/// A sign's `text` fitted to `max` wide: at the largest size up to `sizes.1` at which it
/// fits whole, down to `sizes.0`; at `sizes.0` and cut with "…" when even that is too
/// wide. `width` measures at size 1, and widths grow with the size. Returns the text and
/// its size.
pub fn fit(text: &str, max: f32, sizes: (f32, f32), width: impl Fn(&str) -> f32) -> (String, f32) {
    let (min, largest) = sizes;
    let whole = width(text);
    let size = if whole > 0.0 {
        (max / whole).min(largest)
    } else {
        largest
    };
    if size >= min {
        return (text.to_string(), size);
    }
    (cut(text, max, |s| width(s) * min), min)
}

/// The font's glyphs, read from its file: which characters it has, and how wide a line
/// is (advances only, at the weight given, no kerning). Oswald is a variable font
/// without an `HVAR` table, so its advances at a weight come from its glyph variations,
/// which skrifa (parley's font reader, and so Bevy's) applies.
pub struct Glyphs {
    data: Vec<u8>,
    weight: Location,
}

impl Glyphs {
    /// The font in `data` at `weight` (on its `wght` axis, if it has one); none if
    /// `data` is no font.
    pub fn new(data: Vec<u8>, weight: f32) -> Option<Self> {
        let font = FontRef::new(&data).ok()?;
        let weight = font.axes().location([("wght", weight)]);
        Some(Glyphs { data, weight })
    }

    fn font(&self) -> FontRef<'_> {
        FontRef::new(&self.data).expect("checked in new")
    }

    /// Whether the font draws `c`.
    pub fn has(&self, c: char) -> bool {
        self.font().charmap().map(c).is_some()
    }

    /// How wide `text` is at size 1 (one em): its glyphs' advances.
    pub fn width(&self, text: &str) -> f32 {
        let font = self.font();
        let charmap = font.charmap();
        let metrics = GlyphMetrics::new(&font, Size::new(1.0), &self.weight);
        text.chars()
            .map(|c| {
                let glyph = charmap.map(c).unwrap_or_default();
                metrics.advance_width(glyph).unwrap_or_default()
            })
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_company_comes_from_the_model_ids_prefix() {
        let of = |id| Company::of(Some(id));
        assert_eq!(of("claude-opus-5-5"), Company::Anthropic);
        assert_eq!(of("claude-haiku-5-5"), Company::Anthropic);
        assert_eq!(of("gpt-6.1-sol"), Company::OpenAi);
        assert_eq!(of("gpt-6-luna"), Company::OpenAi);
        assert_eq!(of("grok-5"), Company::XAi);
        assert_eq!(of("gemini-3-pro"), Company::Google);
    }

    #[test]
    fn openais_o_series_is_o_and_a_number_then_the_end_or_a_dash() {
        let of = |id| Company::of(Some(id));
        for id in [
            "o1",
            "o3",
            "o3-mini",
            "o4-mini",
            "o1-preview",
            "o3-pro-2025-06-10",
        ] {
            assert_eq!(of(id), Company::OpenAi, "{id}");
        }
        for id in [
            "o", "o-mini", "o3x", "omni-1", "opus-5", "ollama-3", "olmo-2",
        ] {
            assert_eq!(of(id), Company::Unknown, "{id}");
        }
    }

    #[test]
    fn any_other_model_or_none_is_unknown() {
        let of = |id| Company::of(Some(id));
        for id in [
            "mystery-7",
            "deepseek-flash",
            "chatgpt-4o-latest",
            "",
            "claude",
            "grok",
        ] {
            assert_eq!(of(id), Company::Unknown, "{id:?}");
        }
        assert_eq!(
            of("Claude-opus-5-5"),
            Company::Unknown,
            "ids are lower case"
        );
        assert_eq!(Company::of(None), Company::Unknown);
    }

    #[test]
    fn the_display_name_never_decides_the_company() {
        let face = Face::of(Some("mystery-7"), Some("Opus 5.5"));
        assert_eq!(face.company, Company::Unknown);
        assert_eq!(
            Face::of(None, Some("GPT-6.1 Sol")).company,
            Company::Unknown
        );
    }

    #[test]
    fn each_company_has_its_own_colour_and_unknown_is_a_neutral_grey() {
        let all = [
            Company::Anthropic,
            Company::OpenAi,
            Company::XAi,
            Company::Google,
            Company::Unknown,
        ];
        for (n, a) in all.iter().enumerate() {
            for b in &all[n + 1..] {
                assert_ne!(a.colour(), b.colour(), "{a:?} and {b:?}");
            }
        }
        let [r, g, b] = Company::Unknown.colour();
        assert!(r == g && g == b, "grey");
    }

    #[test]
    fn a_face_names_its_model_or_nothing_before_one_is_known() {
        assert_eq!(
            Face::of(Some("claude-opus-5-5"), Some("Opus 5.5")),
            Face {
                company: Company::Anthropic,
                name: Some("Opus 5.5".into())
            }
        );
        assert_eq!(
            Face::of(Some("mystery-7"), Some("mystery-7")),
            Face {
                company: Company::Unknown,
                name: Some("mystery-7".into())
            },
            "an unknown model shows its id as it is"
        );
        assert_eq!(
            Face::of(Some("grok-5"), None).name.as_deref(),
            Some("grok-5")
        );
        assert_eq!(
            Face::of(None, None),
            Face {
                company: Company::Unknown,
                name: None
            }
        );
    }

    /// Every character one unit wide.
    fn chars(s: &str) -> f32 {
        s.chars().count() as f32
    }

    #[test]
    fn a_name_that_fits_stays_whole() {
        assert_eq!(cut("Opus 5.5", 8.0, chars), "Opus 5.5");
        assert_eq!(cut("", 0.0, chars), "");
    }

    #[test]
    fn a_name_too_long_is_cut_with_an_ellipsis_within_the_width() {
        assert_eq!(cut("GPT-6.1 Sol", 8.0, chars), "GPT-6.1…");
        assert_eq!(cut("abcdef", 3.0, chars), "ab…");
        // No space before the mark.
        assert_eq!(cut("GPT-6.1 Sol", 9.0, chars), "GPT-6.1…");
        assert_eq!(
            cut("Äpfel und Öl", 6.0, chars),
            "Äpfel…",
            "cut between characters"
        );
    }

    #[test]
    fn a_name_with_no_room_is_the_ellipsis_alone() {
        assert_eq!(cut("abc", 1.0, chars), "…");
        assert_eq!(cut("abc", 0.0, chars), "…");
    }

    #[test]
    fn a_sign_shrinks_a_long_name_to_fit_then_cuts_it_at_the_minimum_size() {
        // 10 wide at most, sizes 1 to 2.
        assert_eq!(fit("Alpha", 10.0, (1.0, 2.0), chars), ("Alpha".into(), 2.0));
        assert_eq!(fit("AgentGamified", 10.0, (0.5, 2.0), chars).1, 10.0 / 13.0);
        assert_eq!(
            fit("AgentGamified", 10.0, (1.0, 2.0), chars),
            ("AgentGami…".into(), 1.0)
        );
        assert_eq!(fit("", 10.0, (1.0, 2.0), chars), (String::new(), 2.0));
    }

    #[test]
    fn characters_the_font_lacks_show_as_question_marks() {
        let latin = |c: char| (c as u32) < 0x250;
        assert_eq!(known("Straße", latin), "Straße");
        assert_eq!(known("项目 one", latin), "?? one");
        assert_eq!(known(" a\tb\n ", latin), "a?b");
    }

    fn oswald() -> Glyphs {
        let file = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../world-assets/third-party/oswald/Oswald[wght].ttf"
        );
        Glyphs::new(std::fs::read(file).unwrap(), 700.0).unwrap()
    }

    #[test]
    fn oswald_has_latin_extended_but_not_chinese() {
        let font = oswald();
        for c in "AZaz09?…ÄÖÜßéøłŁčřżŞĞ-.".chars() {
            assert!(font.has(c), "{c}");
        }
        assert!(!font.has('项'));
        assert!(!font.has('🙂'));
    }

    #[test]
    fn oswald_measures_longer_text_wider_and_bold_wider_than_light() {
        let bold = oswald();
        let light = Glyphs::new(bold.data.clone(), 200.0).unwrap();
        assert!(bold.width("AgentGamified") > bold.width("Agent"));
        assert!(bold.width("AgentGamified") > light.width("AgentGamified") * 1.1);
        let em = bold.width("M");
        assert!(em > 0.3 && em < 1.0, "an M is {em} em");
    }
}
