# ADR-005: Overlay frames

## Context

The HUD lines, the settings popover, the world map and the editor's panes
are each drawn by hand with their own placement and key handling. The
inset view from ADR-004, an inventory, stats, conversations and a
history log would each add another. One frame system should own them.

## Decision

A `Frame` is a rectangle over the scene with:

| Field | Meaning |
|---|---|
| name | key for settings and assets |
| title | shown in the top border |
| anchor | top-left, top-right, bottom-left, bottom-right, top, bottom, left, right, centre, full |
| size | cells, or a fraction of the screen per axis; minimum in cells |
| border | none, line, double, or the chrome style |
| background | opaque chrome, or a tint over the scene with a strength |
| z | draw order; focused frames draw last |
| priority | which frames survive on small screens |
| show | rule: always, when zoomed out, when zoomed in, on key, never below N columns |
| key | toggle key from the binding table |

`Content` is a trait: `fn draw(&self, cv: &mut Canvas, rect, ctx)` into the
frame's canvas and `fn input(&mut self, action) -> Flow` when focused.
Built-in contents: HUD text, settings form, world map, inset view (a
second Camera and Renderer over the same Scene at the other end of the
zoom scale), list (inventory, stats, history), text (conversation with
wrapped lines and a prompt), and the editor's tables, rows, previews,
form and grid.

`Layout` takes the screen size and the frame list, places each frame by
anchor and size, resolves overlaps by priority, hides frames whose rule
fails, and returns the rectangles. It runs each frame on resize or when
a show rule changes.

Frames are declared in `assets/ui.toml` (name, title, anchor, size,
border, background, z, priority, show, key) with content named by kind;
code supplies the content kinds. The game and the editor load different
frame sets from the same table shape.

Focus: at most one frame is focused. A focused frame receives actions
first; Escape returns focus to the scene. Passive frames only draw.

## Consequences

- HUD, popover and world map move onto frames; the golden frames stay
  within tolerance because their pixels do not change.
- The inset view, inventory, stats, conversation and history are rows in
  ui.toml plus a content kind each.
- The editor's panes become frames; its 80x25 collapse becomes priority
  and show rules rather than bespoke layout code.
- One keybinding table gains a `Toggle(frame)` action.

## Implementation plan

1. `src/frame.rs`: Frame, Content, Layout, and the chrome drawing.
2. Move HUD, popover and world map onto it without pixel changes.
3. `assets/ui.toml` and the loader schema; toggle keys in the binding
   table.
4. Inset view content per ADR-004.
5. List and text contents with scrolling; inventory, stats and history
   frames as rows; conversation frame with a prompt line.
6. Editor panes onto frames.
