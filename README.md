# openLayout

openLayout is a scrolling window manager for macOS. Windows sit in columns on a horizontal strip that can extend past the screen. New windows keep their width, and the strip grows. A grid tiler would shrink every window to fit; this one scrolls instead.

The layout engine is Rust, and the menu-bar app talks to AppKit and the Accessibility API in the same process. Layout math stays free of AppKit so it can be tested without moving windows.

## What it does

- One horizontal strip of columns per display, updated together on every frame.
- Stacks, tabbed columns, width presets, gaps, and a dock-aware usable area.
- Pins on the left or right edge. Those columns stay put while the rest of the strip scrolls.
- Focus border and dimming for inactive windows, both off until you turn them on.
- Horizontal trackpad scrolling, with or without Control-Option held.
- Rebindable Control-Option shortcuts.
- Session restore, launch at login, and per-app float rules.
- Dialogs, tiny windows, and a few system apps stay floating.

Launching the app does not move anything. Choose **Start tiling** in the menu bar when you want it to take over. The first launch does not ask for Accessibility until you start tiling or choose **Grant Accessibility permission**.

## Several displays

**Independent strips** is the default. Each display has its own column sequence, its own scroll position, and its own pins. Focus and move commands can cross to the next display. Columns that leave the screen are parked off every display, so they do not spill onto the monitor beside them. A column that would cross a bezel is cropped to the display that owns it. Empty space past the last display is left alone.

**Unified desk** is one column sequence that flows from the left display to the right. The left pin lives on the leftmost display and the right pin on the rightmost. Width ratios are fractions of the usable width of the leftmost display.

**Linked scroll** keeps independent strips, and applies the same scroll delta to every display.

**Overview** (Control-Option-O) draws every display. Drag a column onto another screen to move it.

**Profiles** store a named arrangement of apps, widths, stacks, pins, and display slots. Control-Option-\\ cycles them. The same binary accepts `openlayout profile save NAME` and `openlayout profile apply NAME`.

## Install

Download the latest disk image from [GitHub Releases](https://github.com/angeeelvega/openLayout/releases/latest). Open it, drag **OpenLayout** onto **Applications**, then eject the disk.

The app lives in the menu bar, not the Dock. The download is not notarized, so the first launch needs a manual open: Control-click `OpenLayout` in Applications and choose **Open**. Then choose **Grant Accessibility permission** in the menu, and press **Start tiling** when you want windows to move.

To build the disk image yourself:

```sh
./scripts/build-dmg.sh
```

## Requirements

- macOS 13 or later
- Rust stable, if you build from source
- Accessibility permission, granted when you start tiling

An ad-hoc signature is enough for a local build. Launch at login works for the bundled `.app`.

## Build from source

```sh
./scripts/build-app.sh
open OpenLayout.app
```

`build-app.sh` builds the release binary, wraps it in `OpenLayout.app`, and signs it ad hoc. `cargo test` runs the layout engine without touching the window server.

State is stored at `~/Library/Application Support/openLayout/state.json`.

## Shortcuts

Defaults use Control-Option. Arrow keys and H J K L both move focus. Add Shift to move the column. Settings can replace any binding.

| Shortcut | Action |
| --- | --- |
| ⌃⌥ ← → ↑ ↓ or H L K J | Focus |
| ⌃⌥⇧ ← → ↑ ↓ or H L K J | Move |
| ⌃⌥⌘ ← / → | First / last column |
| ⌃⌥ 1–9 | Jump to a column |
| ⌃⌥ N | Send to the next display |
| ⌃⌥ S | Focus the next display |
| ⌃⌥ Q | Pause or resume |
| ⌃⌥ R | Cycle width |
| ⌃⌥ F | Full width |
| ⌃⌥ C | Center the column |
| ⌃⌥ = / - | Wider / narrower |
| ⌃⌥⇧ = / - | Taller / shorter |
| ⌃⌥ 0 | Equalize heights |
| ⌃⌥ P / ⌃⌥⇧ P | Pin left / pin right |
| ⌃⌥ ] / [ | Stack into the left / right column |
| ⌃⌥⇧ ] / [ | Pop out right / left |
| ⌃⌥⇧ T | Toggle tabs |
| ⌃⌥ T | Float or tile |
| ⌃⌥ O | Overview |
| ⌃⌥ \\ | Next profile |

## Commands

With the app already running, the same binary posts a command instead of opening a second instance:

```sh
openlayout pause
openlayout resume
openlayout toggle
openlayout overview
openlayout focus-next-display
openlayout mode independent
openlayout mode unified
openlayout profile save Writing
openlayout profile apply Writing
```

These URLs do the same work: `openlayout://toggle`, `openlayout://pause`, `openlayout://resume`, `openlayout://overview`, `openlayout://focus-next-display`, `openlayout://mode/unified`, `openlayout://mode/independent`, `openlayout://profile/save/Writing`, `openlayout://profile/apply/Writing`.

## License

MIT. See [LICENSE](LICENSE).
