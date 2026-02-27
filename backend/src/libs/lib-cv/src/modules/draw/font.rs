use std::sync::LazyLock;

use ab_glyph::FontArc;
pub enum FontType {
    RobotoRegular,
    RobotoBlack,
    RobotoBlackItalic,
    RobotoBold,
    RobotoBoldItalic,
    RobotoLight,
    RobotoLightItalic,
    RobotoMedium,
    RobotoMediumItalic,
    RaMono,
    SavedByZero,
}

impl FontType {
    /// Returns a reference to the corresponding FontArc based on the enum variant
    pub fn get_font(&self) -> &'static FontArc {
        match self {
            FontType::RobotoRegular => &ROBO_REGULAR_FONT,
            FontType::RobotoBlack => &ROBO_BLACK_FONT,
            FontType::RobotoBlackItalic => &ROBO_BLACK_ITALIC_FONT,
            FontType::RobotoBold => &ROBO_BOLD_FONT,
            FontType::RobotoBoldItalic => &ROBO_BOLD_ITALIC_FONT,
            FontType::RobotoLight => &ROBO_LIGHT_FONT,
            FontType::RobotoLightItalic => &ROBO_LIGHT_ITALIC_FONT,
            FontType::RobotoMedium => &ROBO_MEDIUM_FONT,
            FontType::RobotoMediumItalic => &ROBO_MEDIUM_ITALIC_FONT,
            FontType::RaMono => &RAMONO_FONT,
            FontType::SavedByZero => &SAVED_BY_ZERO_FONT,
        }
    }
}

// Embed the Roboto fonts using include_bytes! and Lazy initialization

// Roboto Regular
static ROBO_REGULAR_FONT: LazyLock<FontArc> = LazyLock::new(|| FontArc::try_from_slice(include_bytes!("fonts/roboto/Roboto-Regular.ttf")).expect("Failed to load Roboto-Regular font"));

// Roboto Black
static ROBO_BLACK_FONT: LazyLock<FontArc> = LazyLock::new(|| FontArc::try_from_slice(include_bytes!("fonts/roboto/Roboto-Black.ttf")).expect("Failed to load Roboto-Black font"));

// Roboto Black Italic
static ROBO_BLACK_ITALIC_FONT: LazyLock<FontArc> = LazyLock::new(|| FontArc::try_from_slice(include_bytes!("fonts/roboto/Roboto-BlackItalic.ttf")).expect("Failed to load Roboto-BlackItalic font"));

// Roboto Bold
static ROBO_BOLD_FONT: LazyLock<FontArc> = LazyLock::new(|| FontArc::try_from_slice(include_bytes!("fonts/roboto/Roboto-Bold.ttf")).expect("Failed to load Roboto-Bold font"));

// Roboto Bold Italic
static ROBO_BOLD_ITALIC_FONT: LazyLock<FontArc> = LazyLock::new(|| FontArc::try_from_slice(include_bytes!("fonts/roboto/Roboto-BoldItalic.ttf")).expect("Failed to load Roboto-BoldItalic font"));

// Roboto Light
static ROBO_LIGHT_FONT: LazyLock<FontArc> = LazyLock::new(|| FontArc::try_from_slice(include_bytes!("fonts/roboto/Roboto-Light.ttf")).expect("Failed to load Roboto-Light font"));

// Roboto Light Italic
static ROBO_LIGHT_ITALIC_FONT: LazyLock<FontArc> = LazyLock::new(|| FontArc::try_from_slice(include_bytes!("fonts/roboto/Roboto-LightItalic.ttf")).expect("Failed to load Roboto-LightItalic font"));

// Roboto Medium
static ROBO_MEDIUM_FONT: LazyLock<FontArc> = LazyLock::new(|| FontArc::try_from_slice(include_bytes!("fonts/roboto/Roboto-Medium.ttf")).expect("Failed to load Roboto-Medium font"));

// Roboto Medium Italic
static ROBO_MEDIUM_ITALIC_FONT: LazyLock<FontArc> = LazyLock::new(|| FontArc::try_from_slice(include_bytes!("fonts/roboto/Roboto-MediumItalic.ttf")).expect("Failed to load Roboto-MediumItalic font"));

// Embed other fonts as before

// Ra Mono font using include_bytes! and LazyLock initialization
static RAMONO_FONT: LazyLock<FontArc> = LazyLock::new(|| FontArc::try_from_slice(include_bytes!("fonts/ra_mono/Ra-Mono.otf")).expect("Failed to load Ra Mono font"));

// Saved by Zero font using include_bytes! and LazyLock initialization
static SAVED_BY_ZERO_FONT: LazyLock<FontArc> = LazyLock::new(|| FontArc::try_from_slice(include_bytes!("fonts/saved_by_zero/Saved by Zero Rg.otf")).expect("Failed to load Saved by Zero font"));
