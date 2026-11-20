import { cn } from "@/lib/utils"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"

export type Option = { value: string; label: string; hint?: string }

type Props = {
  id?: string
  value: string
  onChange: (value: string) => void
  /** Called each time the dropdown opens, e.g. to refresh a stale list. */
  onOpen?: () => void
  options: Option[]
  placeholder?: string
  disabled?: boolean
  className?: string
}

export function OptionSelect({ id, value, onChange, onOpen, options, placeholder, disabled, className }: Props) {
  return (
    <Select
      value={value}
      onValueChange={(v) => v != null && onChange(String(v))}
      onOpenChange={(open) => open && onOpen?.()}
      items={options}
      disabled={disabled}
    >
      <SelectTrigger id={id} className={cn("w-full", className)}>
        <SelectValue placeholder={placeholder} />
      </SelectTrigger>
      <SelectContent>
        {options.map((o) => (
          <SelectItem key={o.value} value={o.value}>
            <span className="min-w-0 flex-1 truncate">{o.label}</span>
            {o.hint && <span className="ml-auto shrink-0 pl-3 text-xs text-muted-foreground">{o.hint}</span>}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  )
}
