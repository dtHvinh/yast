import { useCallback, useEffect, useMemo, useState } from "react"
import { AlertCircle, ArrowRight, Info, Play, RefreshCw, Square } from "lucide-react"
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Slider } from "@/components/ui/slider"
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"
import { LiveFeed } from "@/components/live-feed"
import { OptionSelect, type Option } from "@/components/option-select"
import { TitleBar } from "@/components/title-bar"
import {
  listOcrLanguages,
  listWindows,
  onEvent,
  startCapture,
  stopCapture,
  type OcrLang,
  type WindowInfo,
} from "@/lib/tauri"
import { useStored } from "@/lib/use-stored"

const REGIONS = [
  { id: "full", label: "Whole window", rect: [0, 0, 1, 1] },
  { id: "bottom40", label: "Bottom 40%", rect: [0, 0.6, 1, 0.4] },
  { id: "bottom25", label: "Bottom 25%", rect: [0, 0.75, 1, 0.25] },
  { id: "middle", label: "Middle", rect: [0, 0.25, 1, 0.5] },
  { id: "top40", label: "Top 40%", rect: [0, 0, 1, 0.4] },
] as const

const TARGETS: Option[] = [
  ["en", "English"], ["vi", "Vietnamese"], ["es", "Spanish"], ["fr", "French"], ["de", "German"],
  ["pt", "Portuguese"], ["ru", "Russian"], ["it", "Italian"], ["id", "Indonesian"], ["th", "Thai"],
  ["ja", "Japanese"], ["ko", "Korean"], ["zh-CN", "Chinese (Simplified)"], ["zh-TW", "Chinese (Traditional)"],
].map(([value, label]) => ({ value, label }))

const PROVIDERS = [
  { id: "google", label: "Google", note: "Free, no key. Google may rate limit your connection." },
  { id: "mymemory", label: "MyMemory", note: "Free, no key. Lower quality, and limited daily volume." },
  { id: "deepl", label: "DeepL", note: "Best quality. Needs an API key (a free plan exists)." },
  { id: "libre", label: "Libre", note: "Your own LibreTranslate server. Can run fully offline." },
] as const

type Notice = { kind: "error" | "info"; text: string } | null

export default function App() {
  const [windows, setWindows] = useState<WindowInfo[]>([])
  const [langs, setLangs] = useState<OcrLang[]>([])
  const [hwnd, setHwnd] = useState("")
  const [running, setRunning] = useState(false)
  const [busy, setBusy] = useState(false)
  const [notice, setNotice] = useState<Notice>(null)

  const [source, setSource] = useStored("source", "auto")
  const [target, setTarget] = useStored("target", "en")
  const [regionId, setRegionId] = useStored("regionId", "bottom40")
  const [interval, setIntervalMs] = useStored("interval", 500)
  const [provider, setProvider] = useStored("provider", "google")
  const [deeplKey, setDeeplKey] = useStored("deeplKey", "")
  const [libreUrl, setLibreUrl] = useStored("libreUrl", "")
  const [libreKey, setLibreKey] = useStored("libreKey", "")

  const refreshWindows = useCallback(async () => {
    const list = (await listWindows()).sort((a, b) => a.title.localeCompare(b.title))
    setWindows(list)
    setHwnd((current) => (list.some((w) => String(w.hwnd) === current) ? current : ""))
  }, [])

  useEffect(() => {
    void refreshWindows()
    void listOcrLanguages().then((found) => {
      setLangs(found)
      if (found.length === 0) {
        setNotice({
          kind: "error",
          text: "No OCR languages are installed. Add one in Windows Settings > Time & language > Language & region.",
        })
      }
    })
    const subs = [
      onEvent<string>("status-error", (text) => setNotice({ kind: "error", text })),
      onEvent("capture-ended", () => {
        setRunning(false)
        setNotice({ kind: "info", text: "The window was closed, so translation stopped." })
      }),
    ]
    return () => subs.forEach((s) => void s.then((off) => off()))
  }, [refreshWindows])

  const windowOptions = useMemo<Option[]>(
    () => windows.map((w) => ({ value: String(w.hwnd), label: w.title, hint: w.process.replace(/\.exe$/i, "") })),
    [windows],
  )
  const sourceOptions = useMemo<Option[]>(
    () => [{ value: "auto", label: "Auto-detect" }, ...langs.map((l) => ({ value: l.tag, label: l.name }))],
    [langs],
  )
  const providerNote = PROVIDERS.find((p) => p.id === provider)?.note

  async function toggle() {
    setBusy(true)
    try {
      if (running) {
        await stopCapture()
        setRunning(false)
        setNotice(null)
      } else {
        const region = REGIONS.find((r) => r.id === regionId) ?? REGIONS[0]
        await startCapture(Number(hwnd), {
          sourceLang: source,
          targetLang: target,
          provider,
          deeplKey,
          libreUrl,
          libreKey,
          intervalMs: interval,
          region: [...region.rect],
        })
        setRunning(true)
        setNotice(null)
      }
    } catch (e) {
      setNotice({ kind: "error", text: String(e) })
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="flex h-screen flex-col">
      <TitleBar>
        <Badge variant={running ? "default" : "secondary"} className="gap-1.5">
          <span className="relative flex size-2">
            {running && <span className="absolute inline-flex size-full animate-ping rounded-full bg-current opacity-60" />}
            <span className="relative inline-flex size-2 rounded-full bg-current" />
          </span>
          {running ? "Live" : "Idle"}
        </Badge>
      </TitleBar>
      <div className="min-h-0 flex-1 overflow-y-auto">
        <main className="mx-auto flex max-w-md flex-col gap-4 p-4 pb-8">
          {notice && (
            <Alert variant={notice.kind === "error" ? "destructive" : "default"}>
              {notice.kind === "error" ? <AlertCircle /> : <Info />}
              <AlertTitle>{notice.kind === "error" ? "Something went wrong" : "Heads up"}</AlertTitle>
              <AlertDescription>{notice.text}</AlertDescription>
            </Alert>
          )}

          <Card size="sm">
            <CardHeader>
              <CardTitle>Source</CardTitle>
              <CardDescription>Which window to read, and where its text appears.</CardDescription>
            </CardHeader>
            <CardContent className="space-y-4">
              <div className="space-y-2">
                <Label htmlFor="window">Window</Label>
                <div className="flex gap-2">
                  <OptionSelect
                    id="window"
                    value={hwnd}
                    onChange={setHwnd}
                    onOpen={() => void refreshWindows()}
                    options={windowOptions}
                    placeholder="Choose a window…"
                    disabled={running}
                    className="min-w-0 flex-1"
                  />
                  <Button
                    variant="outline"
                    size="icon"
                    onClick={() => void refreshWindows()}
                    disabled={running}
                    aria-label="Refresh window list"
                    title="Refresh window list"
                  >
                    <RefreshCw />
                  </Button>
                </div>
              </div>

              <div className="space-y-2">
                <Label>Text area</Label>
                <ToggleGroup
                  variant="outline"
                  size="sm"
                  spacing={1}
                  value={[regionId]}
                  onValueChange={(v) => v.length > 0 && setRegionId(String(v[0]))}
                  disabled={running}
                  className="flex-wrap"
                >
                  {REGIONS.map((r) => (
                    <ToggleGroupItem key={r.id} value={r.id}>
                      {r.label}
                    </ToggleGroupItem>
                  ))}
                </ToggleGroup>
                <p className="text-xs text-muted-foreground">
                  Limiting the area to where subtitles appear is faster and more accurate.
                </p>
              </div>
            </CardContent>
          </Card>

          <Card size="sm">
            <CardHeader>
              <CardTitle>Languages</CardTitle>
            </CardHeader>
            <CardContent>
              <div className="grid grid-cols-[1fr_auto_1fr] items-end gap-2">
                <div className="min-w-0 space-y-2">
                  <Label htmlFor="source">Game text</Label>
                  <OptionSelect id="source" value={source} onChange={setSource} options={sourceOptions} disabled={running} />
                </div>
                <ArrowRight className="mb-2 size-4 text-muted-foreground" />
                <div className="min-w-0 space-y-2">
                  <Label htmlFor="target">Translate to</Label>
                  <OptionSelect id="target" value={target} onChange={setTarget} options={TARGETS} disabled={running} />
                </div>
              </div>
            </CardContent>
          </Card>

          <Card size="sm">
            <CardHeader>
              <CardTitle>Translation</CardTitle>
              <CardDescription>{providerNote}</CardDescription>
            </CardHeader>
            <CardContent className="space-y-4">
              <Tabs value={provider} onValueChange={(v) => setProvider(String(v))}>
                <TabsList className="w-full">
                  {PROVIDERS.map((p) => (
                    <TabsTrigger key={p.id} value={p.id} disabled={running}>
                      {p.label}
                    </TabsTrigger>
                  ))}
                </TabsList>
              </Tabs>

              {provider === "deepl" && (
                <div className="space-y-2">
                  <Label htmlFor="deeplKey">DeepL API key</Label>
                  <Input
                    id="deeplKey"
                    type="password"
                    autoComplete="off"
                    placeholder="xxxxxxxx-xxxx-…:fx"
                    value={deeplKey}
                    onChange={(e) => setDeeplKey(e.target.value)}
                    disabled={running}
                  />
                </div>
              )}
              {provider === "libre" && (
                <div className="grid gap-4">
                  <div className="space-y-2">
                    <Label htmlFor="libreUrl">Server URL</Label>
                    <Input
                      id="libreUrl"
                      placeholder="http://localhost:5000"
                      value={libreUrl}
                      onChange={(e) => setLibreUrl(e.target.value)}
                      disabled={running}
                    />
                  </div>
                  <div className="space-y-2">
                    <Label htmlFor="libreKey">API key (optional)</Label>
                    <Input
                      id="libreKey"
                      type="password"
                      autoComplete="off"
                      value={libreKey}
                      onChange={(e) => setLibreKey(e.target.value)}
                      disabled={running}
                    />
                  </div>
                </div>
              )}

              <div className="space-y-3">
                <div className="flex items-center justify-between">
                  <Label>Scan interval</Label>
                  <span className="text-sm text-muted-foreground tabular-nums">{interval} ms</span>
                </div>
                <Slider
                  min={200}
                  max={2000}
                  step={100}
                  value={[interval]}
                  onValueChange={(v) => setIntervalMs(Array.isArray(v) ? v[0] : v)}
                  disabled={running}
                  aria-label="Scan interval"
                />
                <p className="text-xs text-muted-foreground">Lower is more responsive but uses more CPU.</p>
              </div>
            </CardContent>
          </Card>

          <Button
            size="lg"
            variant={running ? "destructive" : "default"}
            className="h-11 w-full text-sm font-semibold"
            disabled={busy || (!running && !hwnd)}
            onClick={() => void toggle()}
          >
            {running ? <Square /> : <Play />}
            {running ? "Stop translating" : hwnd ? "Start translating" : "Choose a window to start"}
          </Button>

          <LiveFeed running={running} />

          <p className="flex gap-2 text-xs text-muted-foreground">
            <Info className="mt-0.5 size-3.5 shrink-0" />
            Run the game windowed or borderless. An overlay can't draw over exclusive fullscreen. It shows only while the
            game is the active window.
          </p>
        </main>
      </div>
    </div>
  )
}
