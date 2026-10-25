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
- [ ] Retina and non-Retina screens: the wheel opens under the cursor on either screen, and the highlighted slice matches it.
- [ ] Moving onto another slice ticks the trackpad (Force Touch trackpad, **Haptic feedback** on in Settings).
- [ ] Switch macOS between Light and Dark: the wheel follows.
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

## On-device AI (Vision)
- [ ] ⌘-drop a photo with a clear subject on **Remove BG**: a transparent `photo-cutout.png` appears (macOS 14+ only; the slice is hidden on 13).
- [ ] ⌘-drop a screenshot on **Extract text**, and a scanned PDF on **OCR text**: the `.txt` reads top to bottom, left to right.

## Long jobs
- [ ] Convert a video of a minute or more to WebM: the progress ring fills and a notification appears at the end.
- [ ] A failing job (e.g. a corrupted file) shows "Failed" with a reason.
- [ ] The end-of-job notification has an **Open** button that opens the result; clicking the notification body reveals it in Finder (File Explorer: **Show in folder**).
- [ ] ⌘-drop a long video on **≤ 10 MB**: the result is under 10 MB and plays. Change the target in Settings: the slice label follows.

## Trim and crop
- [ ] ⌘-drop an MP4 on **Trim…**: the editor plays it; drag the handles or press I / O at the playhead, Space plays the kept range only. **Trim** writes `clip-trimmed.mp4` of that length and closes the window.
- [ ] An MKV or AVI (no preview): the timeline still works from typed times.
- [ ] ⌘-drop a photo on **Crop…**: drag the frame and its corners, try 1:1 and 16:9, type X/Y/W/H. A portrait iPhone photo is shown upright and cropped where shown.
- [ ] Esc closes the editor, Enter applies.

## Finder integration
- [ ] Right-click a file → Open With → Clémentine: the wheel opens for it (the app is never offered as the default).
- [ ] `open -a Clémentine a.png b.png` opens the wheel for both.

## Bundled ffmpeg (release builds)
- [ ] With Homebrew's ffmpeg uninstalled, video conversion works; Settings → Engines shows ffmpeg as **Built in**.

## Recipes
- [ ] The default *Web 1080* recipe is first on the Tools wheel for images, absent for videos.
- [ ] Settings → Recipes: name a recipe, pick steps (they show as chips, × removes one), **Add recipe**. It appears on the wheel right away.
- [ ] **Edit** loads a recipe into the editor, **Save** replaces it; **Delete** removes it.
- [ ] Running a recipe leaves only the final file next to the original.

## Settings
- [ ] Change the keys, output folder and quality; restart the app and check they persist.
- [ ] Click the shortcut button and press a new combination (e.g. ⌃⌥K): it is used right away. ⌫ clears it, Esc cancels, and a combination another app owns shows an error and keeps the old one.
- [ ] Untick **Enabled**: dragging no longer opens the wheel. The tray checkbox follows.
- [ ] **Open at login** survives a reboot.
- [ ] **Hide menu bar icon** removes the icon at once and after a restart; opening Clémentine again from Finder or Spotlight shows Settings.
