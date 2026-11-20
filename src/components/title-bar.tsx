import { useEffect, useState, type ReactNode } from "react"
import { getCurrentWindow } from "@tauri-apps/api/window"
import { Copy, Languages, Minus, Square, X } from "lucide-react"
import { cn } from "@/lib/utils"

const appWindow = getCurrentWindow()

function ControlButton({
  label,
  onClick,
  className,
  children,
}: {
  label: string
  onClick: () => void
  className?: string
  children: ReactNode
}) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      onClick={onClick}
      className={cn(
        "inline-flex h-full w-12 items-center justify-center text-muted-foreground transition-colors outline-none hover:bg-white/10 hover:text-foreground focus-visible:bg-white/10 [&_svg]:size-3.5",
        className,
      )}
    >
      {children}
    </button>
  )
}

/**
 * Replaces the native window frame. The bar itself is the drag handle (double-click toggles maximize);
 * its text and icons ignore the pointer so presses fall through to the handle, while the buttons stay clickable.
 */
export function TitleBar({ children }: { children?: ReactNode }) {
  const [maximized, setMaximized] = useState(false)

  useEffect(() => {
    const sync = () => void appWindow.isMaximized().then(setMaximized)
    sync()
    const unlisten = appWindow.onResized(sync)
    return () => void unlisten.then((off) => off())
  }, [])

  return (
    <div
      data-tauri-drag-region
      className="flex h-10 shrink-0 items-center border-b bg-card/60 pl-3 select-none"
    >
      <div className="pointer-events-none flex items-center gap-2">
        <div className="flex size-6 items-center justify-center rounded-md bg-primary text-primary-foreground">
          <Languages className="size-3.5" />
        </div>
        <span className="text-sm font-medium">Screen Translator</span>
      </div>
      <div className="pointer-events-none ml-3">{children}</div>

      <div className="ml-auto flex h-full">
        <ControlButton label="Minimize" onClick={() => void appWindow.minimize()}>
          <Minus />
        </ControlButton>
        <ControlButton label={maximized ? "Restore" : "Maximize"} onClick={() => void appWindow.toggleMaximize()}>
          {maximized ? <Copy className="-scale-x-100" /> : <Square />}
        </ControlButton>
        <ControlButton
          label="Close"
          onClick={() => void appWindow.close()}
          className="hover:bg-red-600 hover:text-white focus-visible:bg-red-600 focus-visible:text-white"
        >
          <X />
        </ControlButton>
      </div>
    </div>
  )
}
