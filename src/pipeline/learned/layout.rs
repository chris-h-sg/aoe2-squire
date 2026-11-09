//! Where the game draws each number of the resource panel, per UI layout.
//!
//! Positions are in the units of the game's `widgetui/resourcepanel.json` (the base game's, or a
//! UI mod's copy of it), with each widget's alignment resolved to an absolute box. On screen one
//! unit is `0.625 * ui_scale` pixels (`ui_scale` from `anchor::detect_ui_scale`), and the font
//! size in pixels is `PointSize * 0.625 * ui_scale`.

/// A number on the resource panel. `key()` gives its place in `Results`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Field {
    WoodVils,
    FoodVils,
    GoldVils,
    StoneVils,
    PopVils,
    IdleVils,
    WoodTotal,
    FoodTotal,
    GoldTotal,
    StoneTotal,
    /// "current/limit", read with the slash.
    PopTotal,
}

impl Field {
    pub const ALL: [Field; 11] = [
        Field::WoodVils,
        Field::FoodVils,
        Field::GoldVils,
        Field::StoneVils,
        Field::PopVils,
        Field::IdleVils,
        Field::WoodTotal,
        Field::FoodTotal,
        Field::GoldTotal,
        Field::StoneTotal,
        Field::PopTotal,
    ];

    /// Name as in `ui_map.json` and the shape files.
    pub fn name(self) -> &'static str {
        match self {
            Field::WoodVils => "wood_vils",
            Field::FoodVils => "food_vils",
            Field::GoldVils => "gold_vils",
            Field::StoneVils => "stone_vils",
            Field::PopVils => "population_vils",
            Field::IdleVils => "idle_vils",
            Field::WoodTotal => "wood_total",
            Field::FoodTotal => "food_total",
            Field::GoldTotal => "gold_total",
            Field::StoneTotal => "stone_total",
            Field::PopTotal => "population_total",
        }
    }

    pub fn from_name(name: &str) -> Option<Field> {
        Field::ALL.into_iter().find(|f| f.name() == name)
    }

    /// Most characters the field can show.
    pub fn max_len(self) -> usize {
        match self {
            Field::PopTotal => 7,
            Field::WoodTotal | Field::FoodTotal | Field::GoldTotal | Field::StoneTotal => 6,
            _ => 3,
        }
    }
}

/// Label type, by outline: text with a black outline (villager counts; the idle count on the
/// Anne_HK bell) or text without one (resource totals and population; the base game's idle count,
/// outline 0.01). The outline changes the glyph edges on screen, so each type has its own shapes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    Count,
    Total,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Count => "count",
            Kind::Total => "total",
        }
    }
}

/// What the text is drawn over, which decides how its coverage is measured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backing {
    /// Thick black outline over the icon's dark box: coverage is measured against black.
    Outlined,
    /// Black outline over a saturated colour with no grey art in the region (the Anne_HK bell):
    /// measured against black like `Outlined`, and every pixel of a grey shade is scored.
    OutlinedOverColour,
    /// Over the panel or an icon (thin or no outline): the background under the text is
    /// estimated from the pixels around it.
    Panel,
}

#[derive(Debug, Clone, Copy)]
pub struct FieldLayout {
    pub field: Field,
    /// Top of the label box (layout units); the pen height is calibrated relative to it.
    pub top: f32,
    /// Region searched for the text, x0, y0, x1, y1 (layout units): the label box plus a small
    /// margin. Villager counts (and the base idle count) span 0.2-0.85 em below the box top: their
    /// digits span about 0.25-0.7 em, and the icon art around them would only add noise. The
    /// totals stop 30 units below their top, above the panel's bright bottom rim.
    pub crop: [f32; 4],
    pub point_size: f32,
    /// Text colour (0-255) when nothing changes it (the population total also turns yellow).
    pub colour: [f32; 3],
    pub backing: Backing,
    /// Label type, which decides the glyph shapes used to read it.
    pub kind: Kind,
    /// Which edge of the box the text is anchored to.
    pub anchor: Anchor,
}

/// Horizontal text anchor within the label box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Anchor {
    Left,
    Right,
    Centre,
}

#[derive(Debug)]
pub struct Layout {
    pub name: &'static str,
    pub fields: [FieldLayout; 11],
}

impl Layout {
    pub fn field(&self, f: Field) -> &FieldLayout {
        self.fields.iter().find(|l| l.field == f).unwrap()
    }

    pub fn by_name(name: &str) -> Option<&'static Layout> {
        [&BASE, &ANNE_HK].into_iter().find(|l| l.name == name)
    }
}

const WHITE: [f32; 3] = [255.0, 255.0, 255.0];

const fn count(
    field: Field,
    x0: f32,
    x1: f32,
    top: f32,
    y: [f32; 2],
    pt: f32,
    colour: [f32; 3],
) -> FieldLayout {
    FieldLayout {
        field,
        top,
        crop: [x0 - 2.0, y[0], x1 + 2.0, y[1]],
        point_size: pt,
        colour,
        backing: Backing::Outlined,
        kind: Kind::Count,
        anchor: Anchor::Right,
    }
}

const fn total(field: Field, x0: f32, x1: f32, top: f32, bottom: f32, pt: f32) -> FieldLayout {
    FieldLayout {
        field,
        top,
        crop: [x0 - 2.0, top - 2.0, x1 + 2.0, bottom],
        point_size: pt,
        colour: WHITE,
        backing: Backing::Panel,
        kind: Kind::Total,
        anchor: Anchor::Left,
    }
}

const fn idle(x0: f32, x1: f32, top: f32, y0: f32, y1: f32, pt: f32) -> FieldLayout {
    FieldLayout {
        field: Field::IdleVils,
        top,
        crop: [x0 - 2.0, y0, x1 + 2.0, y1],
        point_size: pt,
        colour: WHITE,
        backing: Backing::Panel,
        kind: Kind::Total,
        anchor: Anchor::Right,
    }
}

/// The base game's panel (`<AoE2DE>/widgetui/resourcepanel.json`). Villager counts: 32 pt white,
/// outline 0.05, anchored top right in a 60x32 box at (16, 55) inside each 84x84 icon. Totals:
/// 34 pt white, no outline, top left in a 100x34 box centred on y = 60 (top 43; population 130
/// wide). Idle:
/// 32 pt, outline 0.01, top right in a 60x32 box.
pub static BASE: Layout = Layout {
    name: "base",
    fields: [
        count(Field::WoodVils, 31.0, 91.0, 70.0, BASE_COUNT_Y, 32.0, WHITE),
        count(
            Field::FoodVils,
            231.0,
            291.0,
            70.0,
            BASE_COUNT_Y,
            32.0,
            WHITE,
        ),
        count(
            Field::GoldVils,
            431.0,
            491.0,
            70.0,
            BASE_COUNT_Y,
            32.0,
            WHITE,
        ),
        count(
            Field::StoneVils,
            631.0,
            691.0,
            70.0,
            BASE_COUNT_Y,
            32.0,
            WHITE,
        ),
        count(
            Field::PopVils,
            829.0,
            889.0,
            70.0,
            BASE_COUNT_Y,
            32.0,
            WHITE,
        ),
        idle(
            1044.0,
            1104.0,
            56.0,
            56.0 + 0.2 * 32.0,
            56.0 + 0.85 * 32.0,
            32.0,
        ),
        total(Field::WoodTotal, 108.0, 208.0, 43.0, 73.0, 34.0),
        total(Field::FoodTotal, 308.0, 408.0, 43.0, 73.0, 34.0),
        total(Field::GoldTotal, 508.0, 608.0, 43.0, 73.0, 34.0),
        total(Field::StoneTotal, 710.0, 810.0, 43.0, 73.0, 34.0),
        total(Field::PopTotal, 910.0, 1040.0, 43.0, 73.0, 34.0),
    ],
};

/// Vertical search range of the villager counts (layout units).
const BASE_COUNT_Y: [f32; 2] = [70.0 + 0.2 * 32.0, 70.0 + 0.85 * 32.0];
const HK_COUNT_Y: [f32; 2] = [62.0, 94.0];

/// The idle count on the Anne_HK bell: white with a black outline over red (screen pixels are
/// white over black inside the outline, whatever the red beside it), read like a villager count.
const fn hk_idle(x0: f32, x1: f32, top: f32, y0: f32, y1: f32, pt: f32) -> FieldLayout {
    FieldLayout {
        field: Field::IdleVils,
        top,
        crop: [x0 - 2.0, y0, x1 + 2.0, y1],
        point_size: pt,
        colour: WHITE,
        backing: Backing::OutlinedOverColour,
        kind: Kind::Count,
        anchor: Anchor::Centre,
    }
}

const HK_YELLOW: [f32; 3] = [255.0, 214.2, 0.0];
const HK_CYAN: [f32; 3] = [127.5, 255.0, 255.0];

/// The Anne_HK "Better Resource Panel and Idle Villager Icon" mod (its own
/// `widgetui/resourcepanel.json`, same font). Villager counts: 40 pt yellow (population cyan),
/// outline 0.05, top right in a 70x42 box at (4, 45) inside each icon. Totals: 41 pt white, top
/// left in a 110x34 box centred on y = 55 (top 38; population 147 wide). Idle: 39 pt white,
/// black outline as drawn, centred in a 60x32 box over the red bell.
pub static ANNE_HK: Layout = Layout {
    name: "anne_hk",
    fields: [
        count(
            Field::WoodVils,
            19.0,
            89.0,
            60.0,
            HK_COUNT_Y,
            40.0,
            HK_YELLOW,
        ),
        count(
            Field::FoodVils,
            219.0,
            289.0,
            60.0,
            HK_COUNT_Y,
            40.0,
            HK_YELLOW,
        ),
        count(
            Field::GoldVils,
            419.0,
            489.0,
            60.0,
            HK_COUNT_Y,
            40.0,
            HK_YELLOW,
        ),
        count(
            Field::StoneVils,
            619.0,
            689.0,
            60.0,
            HK_COUNT_Y,
            40.0,
            HK_YELLOW,
        ),
        count(
            Field::PopVils,
            818.0,
            888.0,
            60.0,
            HK_COUNT_Y,
            40.0,
            HK_CYAN,
        ),
        hk_idle(1065.0, 1125.0, 45.0, 43.0, 79.0, 39.0),
        total(Field::WoodTotal, 103.0, 213.0, 38.0, 72.0, 41.0),
        total(Field::FoodTotal, 305.0, 415.0, 38.0, 72.0, 41.0),
        total(Field::GoldTotal, 503.0, 613.0, 38.0, 72.0, 41.0),
        total(Field::StoneTotal, 705.0, 815.0, 38.0, 72.0, 41.0),
        total(Field::PopTotal, 908.0, 1055.0, 38.0, 72.0, 41.0),
    ],
};

/// Screen pixels per layout unit.
pub fn px_per_unit(ui_scale: f64) -> f32 {
    (0.625 * ui_scale) as f32
}
