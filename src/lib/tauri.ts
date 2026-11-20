import { invoke } from "@tauri-apps/api/core"
import { listen, type UnlistenFn } from "@tauri-apps/api/event"

export type WindowInfo = { hwnd: number; title: string; process: string }
export type OcrLang = { tag: string; name: string }

export type TranslatedBlock = {
  x: number
  y: number
  w: number
  h: number
  lines: number
  text: string
  translated: string
}

export type OverlayPayload = {
  frameW: number
  frameH: number
  blocks: TranslatedBlock[]
}

export type CaptureConfig = {
  sourceLang: string
  targetLang: string
  provider: string
  deeplKey: string
  libreUrl: string
  libreKey: string
  intervalMs: number
  enhance: boolean
  region: [number, number, number, number]
}

export const listWindows = () => invoke<WindowInfo[]>("list_windows")
export const listOcrLanguages = () => invoke<OcrLang[]>("list_ocr_languages")
export const startCapture = (hwnd: number, config: CaptureConfig) =>
  invoke<void>("start_capture", { hwnd, config })
export const stopCapture = () => invoke<void>("stop_capture")

export function onEvent<T>(name: string, handler: (payload: T) => void): Promise<UnlistenFn> {
  return listen<T>(name, (e) => handler(e.payload))
}
