# Writing style packs

[한국어](style-packs.md)

A style pack (`pack.json`) holds the capture presets per kind, the fixed prompt sentences, the outline, the canvas fill and the check ranges.
The default pack ships with the app; a local pack lives in `%USERPROFILE%\.unity-icon-studio\packs\<id>\pack.json` and is laid over the default one with `"extends": "default"`.

## Keeping the original is the default

An icon must be the capture **repainted**, not redesigned.

| May change | Must not change |
|---|---|
| Material smoothness (clay, glossy plastic), shading and light, softness of color (a little brighter and cleaner), slightly rounder edges | Which parts there are, how many, where, how big relative to the whole, how thick, their patterns (grids, panes, stripes, planks), the shape of the base or ground |
| Additions the person asks for in the note (for example "snow on the roof") | Sparkles, stars, hearts, flowers, leaves, props, emblems, text or effects that the capture does not have |

The pack's `fidelity` (or the setting `generate.fidelity`, or the per-run choice "Keep the original / Stylize") picks the way.

- **`strict` (keep the original, default):** only the left column changes. No style references are sent (pictures with charms and proportions of their own get copied).
- **`stylized`:** may enlarge key parts, drop fine patterns and add small sparkles or hearts. The pack's `styleRefs` are sent. Meant for showcase art, not for in-game icons that must match the model.

A sentence takes a condition such as `"when": { "fidelity": "strict" }`, so one pack holds both. Condition keys: `kind`, `refs`, `outline`, `background`, `fidelity`.

## Writing sentences

- **Lock** (`"locked": true`) what must never change: the goal (repaint), the keep-every-part rule, the camera, "add nothing", the output. A note only replaces unlocked sentences or adds sentences.
- **Say what to keep, not how to improve.** Words such as "enlarge", "exaggerate", "simplify", "chibi", "charm" and "sparkle" invite changes the capture does not have.
- **Describe the base as captured** ("keep its base exactly as captured"). Describing an ideal base changes the base.
- **Keep colors relative to the capture** ("a little richer than the capture"). A fixed color ("deep green") recolors the whole picture.
- References show the rendering only. Pick ones without charms, and leave them out of `strict`.
- `bannedTerms` lists words that must never reach the prompt, such as product or character names.

## Additions the note asks for

The rewrite model returns what the note asks to add as `additions` (short English noun phrases). Code puts "Except what the note asks for: …" right after the locked
"add nothing" sentence and gives the list to the fidelity review as allowed. Backgrounds, text and logos are still left out.

## Checks, review and retry

A `strict` picture goes through these after post-processing. Limits are in the pack's `checks.strict`.

| Step | What it looks at | Verdict |
|---|---|---|
| Fidelity measures | After matching area and centroid with the capture: row and column silhouette widths (`fidelityProfile`), saturation and brightness change from the capture (`deltaSaturation`, `deltaValue`) | Fails outside the limits |
| | Color spots the capture does not have nearby (`foreignBlobs`), pieces outside the capture's silhouette (`outsideParts`) | At the limit or over: "suspect", the review decides; without a review it fails |
| | Silhouette overlap (IoU) | Fails under `iouMin` (0.92) |
| Fidelity review (`llm.fidelityReview`: `off`, `claude`, `codex`, `both`) | Lists added, resized, missing and re-patterned parts, each `minor` or `major` | Code decides: added (`major`, or two or more), missing (`major`, or two or more), grown/shrunk/thickened/thinned/moved (`major`, or three or more) fail. Reshapes and pattern changes are notes |
| Retry (`generate.fidelityRetries`, 1) | One more picture with a correction sentence at the end of SUBJECT that names only the parts found | Still failing: shown as "differs from the original" and the person picks (export anyway, make it stylized, change the note) |

- Corrections never name colors; a color problem becomes "keep the colors close to the capture, only slightly brighter and cleaner".
- With `both`, Claude and Codex each review and either one failing fails the picture. Codex names resized parts in more detail; Claude is faster.
- A retry uses one more picture of the image limit. No retry after a limit, sign-in or missing-program failure.

## Check ranges

```json
{
  "checks": {
    "default": { "size": [0.82, 0.995], "outlineWidthPx": [3.0, 5.0], "saturation": [0.35, 0.62] },
    "prop": { "fill": [0.25, 0.60] },
    "strict": {
      "iouMin": 0.92, "fidelityProfile": 0.012, "foreignBlobs": 2, "outsideParts": 1,
      "deltaSaturation": [-0.05, 0.15], "deltaValue": [-0.08, 0.15]
    }
  }
}
```

With `strict` the absolute color ranges (`saturation`, `value`) only show values; the capture's own colors are the reference.
