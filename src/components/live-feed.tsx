import { useEffect, useRef, useState } from "react"
import { ArrowDown, Radio } from "lucide-react"
import { Badge } from "@/components/ui/badge"
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card"
import { ScrollArea } from "@/components/ui/scroll-area"
import { onEvent, type OverlayPayload } from "@/lib/tauri"

type FeedItem = { id: number; at: Date; text: string; translated: string }

const MAX_ITEMS = 30

export function LiveFeed({ running }: { running: boolean }) {
  const [items, setItems] = useState<FeedItem[]>([])
  const nextId = useRef(0)

  useEffect(() => {
    const unlisten = onEvent<OverlayPayload>("ocr-result", ({ blocks }) => {
      if (blocks.length === 0) return
      setItems((prev) => {
        const fresh = blocks
          .filter((b) => !prev.slice(0, blocks.length).some((p) => p.text === b.text))
          .map((b) => ({ id: nextId.current++, at: new Date(), text: b.text, translated: b.translated }))
        return fresh.length ? [...fresh, ...prev].slice(0, MAX_ITEMS) : prev
      })
    })
    return () => void unlisten.then((fn) => fn())
  }, [])

  return (
    <Card size="sm">
      <CardHeader>
        <CardTitle className="flex items-center justify-between">
          Live feed
          {items.length > 0 && <Badge variant="secondary">{items.length}</Badge>}
        </CardTitle>
        <CardDescription>What was read from the window and how it was translated.</CardDescription>
      </CardHeader>
      <CardContent>
        {items.length === 0 ? (
          <div className="flex h-32 flex-col items-center justify-center gap-2 rounded-lg border border-dashed text-center text-sm text-muted-foreground">
            {running ? <Radio className="size-5 animate-pulse text-primary" /> : <ArrowDown className="size-5" />}
            {running ? "Waiting for text in the window…" : "Start translating to see text appear here."}
          </div>
        ) : (
          <ScrollArea className="h-56">
            <ul className="space-y-2 pr-3">
              {items.map((item) => (
                <li key={item.id} className="rounded-lg border bg-muted/30 px-3 py-2 select-text">
                  <p className="text-sm leading-snug">{item.translated}</p>
                  <p className="mt-1 flex items-baseline justify-between gap-3 text-xs text-muted-foreground">
                    <span className="truncate">{item.text}</span>
                    <time className="shrink-0 tabular-nums">
                      {item.at.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" })}
                    </time>
                  </p>
                </li>
              ))}
            </ul>
          </ScrollArea>
        )}
      </CardContent>
    </Card>
  )
}
