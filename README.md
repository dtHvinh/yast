# Screen Translator

A Tauri app (Windows) that reads text from a game window and shows the translation over it.

## How it works

1. **Capture**: Windows.Graphics.Capture grabs the chosen window. It captures the window itself,
   so the overlay never ends up in the capture.
2. **Change detection**: OCR only runs when the selected region of the window changed.
3. **OCR**: the built-in Windows OCR engine (offline). Needs a Windows language pack for the game's language.
4. **Translate**: Google (free endpoint), MyMemory (free), DeepL (API key) or LibreTranslate (self-host for offline use).
   Results are cached per sentence.
5. **Overlay**: a transparent, click-through, always-on-top window follows the game window and draws the
   translation over the original text. It hides when the game is not the active window.

## Run

```
bun install
bun run dev        # hot-reloading dev build (Vite + Tauri)
bun run build      # installer in src-tauri/target/release/bundle
```

Requirements: Rust, Bun, Node (Vite and the Tauri CLI still run on it), WebView2 and the MSVC build tools.

If the project folder path is long, set `CARGO_TARGET_DIR` to a short path (for example `C:\st-target`).
Windows' 260-character path limit otherwise makes the linker fail.

## Use

- Run the game **windowed or borderless**. Exclusive fullscreen cannot be overlaid.
- Pick the game window, the text language, the target language, and where the text appears
  (limiting OCR to the subtitle area is faster and more accurate).
- To add an OCR language: Windows Settings > Time & language > Language & region > add language.

## Layout

Backend (Rust):

- `src-tauri/src/capture.rs`: window capture, region crop, change detection
- `src-tauri/src/ocr.rs`: Windows OCR and merging lines into sentences
- `src-tauri/src/translate.rs`: translation providers and cache
- `src-tauri/src/session.rs`: worker thread and overlay tracking

Frontend (React, TypeScript, Tailwind v4, shadcn/ui):

- `src/App.tsx`: control panel
- `src/components/live-feed.tsx`: recent translations
- `src/components/ui/`: shadcn/ui components (add more with `bunx shadcn@latest add <name>`)
- `src/lib/tauri.ts`: typed wrappers for the backend commands and events
- `public/overlay.html`: the overlay, a plain page with no build step
