# Learned glyph shapes

The learned-shape matcher (`src/pipeline/learned/`, the default; `--matcher split` selects the older one) reads the resource panel
by rendering candidate numbers from these shapes and keeping the one that fits the pixels best.
Each shape is the coverage of one character as the game actually draws it on screen, learned
from screenshots with known values. The shapes differ from the font atlas (`DECISION_LOG.md`,
"Learned Glyph Shapes").

## Sets

One set per font and panel layout. Anti-Alias Text on and off use the same shapes.

| Set | Layout | Font | Learned from |
|---|---|---|---|
| `base_default` | base | default (`combined`); the "smooth" option draws the same pixels | UI sizes 75-125% in steps of 5 except the 64/75 shot at 100%, which is held out as a test (`test_bench/held_out/`), and both AA settings at 100% showing 104/160 |
| `base_sans` | base | sans-serif (`combined_sansserif`) | UI sizes 75-125% in steps of 5, AA off as well at 75, 100 and 125% |
| `anne_hk_default` | Anne_HK mod | default | UI sizes 75-125% in steps of 5, AA off as well at 75, 100 and 125% |
| `anne_hk_smooth` | Anne_HK mod | smooth | Copies the `base_default` shapes; only its pen heights are fitted (75, 100 and 125%, AA off as well at 100%) |
| `anne_hk_sans` | Anne_HK mod | sans-serif | Copies the `base_sans` shapes; only its pen heights are fitted (75, 100 and 125%, AA off as well at 100%) |

Anne_HK's default font needs its own shapes: the base shapes read it badly. Its smooth and sans
fonts are read better by the base shapes than by shapes learned from the mod's own screenshots.

Each set has two label types, by outline: `count` (villager counts, black outline) and `total`
(resource totals, population and the base game's idle count, no or a very thin outline). The
idle count on the Anne_HK bell is read with the `count` shapes; `learn_shapes` only calibrates its pen height and does not train on it. Text size follows the layout rule (`PointSize * 0.625 *
ui_scale` pixels), so shapes learned at one UI size read the others. Tested on the base font:
shapes learned from 100% alone read every other size from 75% to 125% correctly.

## Format

- `<set>.png`: 8-bit grey, the coverage images (0-255 for 0-1). One row of characters per label
  type. Each pixel is one font-atlas unit (the font's em is 64 units).
- `<set>.json`:
  - `layout`: the panel layout (`base` or `anne_hk`, see `src/pipeline/learned/layout.rs`);
  - `pen_y`: per field, the pen height below the top of its label box, in atlas units;
  - `kinds.count` and `kinds.total`: per character, its rectangle in the PNG (`x`, `y`, `w`, `h`),
    the image origin relative to the pen (`ox`, `oy`, atlas units), and the pen advance to the
    next character (`adv`, atlas units). The advance is measured on the screenshots, because on
    screen it differs from the font's metrics: the slash advances about as far as a digit.

A character a label type never shows in the training screenshots takes the other type's shape.
'/' only occurs in the population total, so counts always borrow it.

## Regenerating

```
cargo run --release --bin learn_shapes -- --game-dir "<Steam>/steamapps/common/AoE2DE"
```

- `--only <set>` learns one set.
- `--out <dir>` writes the files somewhere else.

The screenshots and their values are listed in `training.json`; they live in
`recordings/screenshots/` (not in git). They are the release `font-screenshots-2026-11` of
[chris-h-sg/aoe2-squire-data](https://github.com/chris-h-sg/aoe2-squire-data/releases/tag/font-screenshots-2026-11):

```bash
gh release download font-screenshots-2026-11 -R chris-h-sg/aoe2-squire-data
unzip font_screenshots.zip -d recordings/
```

Each folder in `recordings/screenshots/` is one panel layout and font (`base_sans`,
`anne_hk_default`, ...). Files are named by UI size, with `_no_aa` for Anti-Alias Text off.

The game directory provides the starting point: the font
metrics and atlas in `resources/_common/fonts/`. `base_default` takes about 4 minutes, the others
under a minute. After regenerating, run `cargo test --test vision_fixtures` and compare the
learned matcher's accuracy with `test_bench/snapshots/vision_accuracy_learned.json`.

To add a font or UI mod:
1. Take screenshots that show every digit in both label types (for example the values in
   `training.json`'s `all_digits`), with Anti-Alias Text on and off.
2. Add the mod's layout to `layout.rs` (from its `widgetui/resourcepanel.json`) if it moves or
   resizes the labels.
3. Add a set to `training.json` and to `embedded_sets` in `shapes.rs`. If an existing set's shapes
   already read the new screenshots, give the new set `"shapes_from": "<that set>"` to copy them
   and fit only its pen heights.
