# Brand assets

The mark, the wordmark and the tagline, as the runtime, the README and the
website all use them. **This directory is the source of truth**; nothing
should re-draw or re-colour them.

| File | Use |
|---|---|
| `usai-logo.svg` | the primary horizontal logo, on white or a light background |
| `usai-logo-on-dark.svg`, `usai-logo-white.svg` | the same on a dark background |
| `usai-logo-no-tagline.svg` | where the tagline would be too small to read |
| `usai-logo-mono-dark.svg`, `usai-icon-mono-dark.svg` | single-colour contexts (print, stamps) |
| `usai-icon.svg`, `usai-icon-white.svg` | the mark alone: avatars, small spaces |
| `usai-app-icon.svg`, `png/icon-*.png`, `png/apple-touch-icon.png`, `png/favicon*` | app and browser icons |
| `png/*` | raster fallbacks where SVG is not an option |

**The tagline is "A workload-native application runtime"** — the same
sentence the README opens with. Usai is not an AI runtime; an earlier
generated brand sheet carried "a world where positive AI transforms
humanity", and that line is wrong and is not used anywhere.

**The mark** is a `U` whose left stroke carries through to the bottom, whose
right stroke ends early, and a dot that sits apart from both — a program that
lives only as long as its work requires. The terminal has its own three-row
rendering of it in `crates/usai-cli/src/display.rs` (`logo()`); keep the two
in agreement.

**Colours**: deep teal `#0D6E66` into mint `#2DD4BF`. The CLI uses exactly
those two.

Do not stretch, rotate, recolour, outline, or add effects; keep clear space
around the logo equal to the height of the icon.
