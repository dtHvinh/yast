import { useEffect, useState } from "react"

type Widen<T> = T extends string ? string : T extends number ? number : never

/** useState that survives restarts; falls back to plain state if storage is unavailable. */
export function useStored<T extends string | number>(key: string, initial: T) {
  const [value, setValue] = useState<Widen<T>>(() => {
    try {
      const saved = localStorage.getItem(`st.${key}`)
      if (saved !== null) return (typeof initial === "number" ? Number(saved) : saved) as Widen<T>
    } catch {
      // storage blocked
    }
    return initial as unknown as Widen<T>
  })

  useEffect(() => {
    try {
      localStorage.setItem(`st.${key}`, String(value))
    } catch {
      // storage blocked
    }
  }, [key, value])

  return [value, setValue] as const
}
