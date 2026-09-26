# SharbCut UI — phase 1

## Architecture and boundaries

`ui/app.slint` owns the window, keyboard routing, menus, launch screen and modal
layers. `workspace/workspace.slint` renders the rectangles published by the Rust
Dock tree; `seat.slint` binds each media / preview / inspector / timeline pane to
the existing `Editor` global. Preserve those bindings, pane replacement, resize,
drag/drop, and the engine's model ownership. Some `ui/demo` components are live
inspector dependencies, not disposable examples.

The Rust editing engine, media pipeline, project serialization, effect ids and
document text colours are outside this phase. No placeholder AI actions or fake
media are introduced. Existing Captions / Speak remain the real AI entry points.

## Visual system

Source: `design-reference/配色板.png`, the dark editor concept, and the editable
SharbCut mark. The UI asset is a cropped chrome variant of that mark; reference
originals remain unchanged.

| Role | Dark value / rule |
| --- | --- |
| Window | `#090F1C` |
| Panel / raised / recessed | `#111B2C` / `#182438` / `#0D1524` |
| Field / hover / pressed | `#1C293E` / `#263750` / `#304664` |
| Main / secondary / tertiary text | `#F8FAFC` / `#B0BED0` / `#8192AB` |
| Brand action | `#2563FF`, white label |
| Selected ink / wash | `#8CAFFF` / electric blue at 16% opacity |
| Border / strong border | slate at 15% / 30% opacity, 1px |
| Radius | 3px small marks; 6px chips; 8px fields; 10px panes; 16px dialogs |
| Type | bundled Synonym for UI, Helvetica Neue for technical values |
| Type scale | 10 / 11 / 12 / 13 / 14px; 18px wordmark; 34px launch heading |
| Spacing | 4 / 8 / 16px shell rhythm; existing 6px dense form rows |
| Heights | 52px title bar, 44px pane headers, 28px dense fields |
| Disabled | 45% opacity, no click action; retain explanatory tooltips |
| Focus | blue ring; preserve keyboard input, focus release and shortcuts |
| Motion | short 100–140ms colour transitions; no decorative animation |
| Shadow | modal elevation only: 24px blur / 8px offset; no pane glow |

Use `Theme` tokens, the existing Lucide-based `Glyph` / `Icon` adapter, and shared
primitives. Keep preview pixels on true black. Keep media-kind colours distinct
across bin and timeline; red playhead remains distinguishable from blue selection.
Light mode uses the same geometry and electric-blue brand, with slate text and
cool white surfaces. No additional font or icon dependency is needed.

## Implementation and verification order

1. Tokens, common controls, window wordmark and title bar.
2. Library navigation with original tab state, catalogues and import callbacks.
3. Monitor transport and Inspector hierarchy, preserving all editing gestures.
4. Timeline shell, tool groups, responsive checks and final diff audit.

Build each checkpoint with `cargo build -p concat --locked` from `engine`, after
loading `scripts/setup-dev.ps1`. Launch the resulting executable for each visual
smoke test. Final gate: `cargo check -p concat --locked`, build, real editor window,
navigation, selection, playback, menus, Inspector, timeline and theme checks.
