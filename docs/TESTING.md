# Manual test checklist

The conversion engine has unit tests (`cargo test --workspace`). The parts below
involve real drags, windows and permissions, so they have to be checked by hand.

Launch with `cd app/src-tauri && cargo tauri dev`, or open
`target/release/bundle/macos/Clémentine.app`.

## Drag detection
- [ ] Drag a PNG from Finder and press **⇧**: the Convert wheel opens centred on the cursor.
- [ ] Still dragging, switch to **⌘**, then **⌃**: the wheel changes to Tools, then Actions.
- [ ] Release the key while still dragging: the wheel stays open.
- [ ] Drop on **WEBP**: the hub shows progress, then ✓, and `file.webp` appears next to the original.
- [ ] Drop in the hub, or outside the ring: the wheel closes and nothing happens.
- [ ] Drag without any key: no wheel.
- [ ] Drag text or a link (not a file) with ⇧: no wheel.
- [ ] Hold **⌘** until the drop (Finder only allows *Move* then): the drop is still accepted.
- [ ] Retina and non-Retina screens: the highlighted slice matches the cursor.
- [ ] A full-screen app and another Space: the wheel appears above them.

## Click / keyboard mode
- [ ] Select files in Finder and press **⌥⌘T**. The first time, macOS asks for Automation access to Finder.
- [ ] Arrow keys move the highlight, **Enter** runs, **Tab** / **1–3** switch wheels, **Esc** closes.
- [ ] Clicking outside the ring, or switching apps, closes the wheel.

## Actions
- [ ] AirDrop opens the share sheet.
- [ ] Float opens the shelf. Dragging an item out of it into Finder copies the file.
- [ ] Copy, then ⌘V in Finder, pastes the file.
- [ ] Move… shows a folder picker and moves the file.
- [ ] Copy path, Terminal.

## Long jobs
- [ ] Convert a video of a minute or more to WebM: the progress ring fills and a notification appears at the end.
- [ ] A failing job (e.g. a corrupted file) shows "Failed" with a reason.

## Settings
- [ ] Change the keys, output folder and quality; restart the app and check they persist.
- [ ] Untick **Enabled**: dragging no longer opens the wheel. The tray checkbox follows.
- [ ] **Open at login** survives a reboot.
