import React from 'react'
import { Spinner } from '@renderer/components/ui/spinner'
import { cn } from '@renderer/lib/utils'

export type ConnectionMode = 'off' | 'proxy' | 'tun'

interface OffProxyTunSelectorProps {
  mode: ConnectionMode
  loading?: boolean
  disableProxy?: boolean
  onChange: (mode: ConnectionMode) => void
  className?: string
}

export const OffProxyTunSelector: React.FC<OffProxyTunSelectorProps> = ({
  mode,
  loading = false,
  disableProxy = false,
  onChange,
  className = ''
}) => {
  const segments: {
    id: ConnectionMode
    label: string
    disabled?: boolean
    activeTextClass: string
  }[] = [
    {
      id: 'off',
      label: 'Off',
      activeTextClass: 'text-foreground font-semibold'
    },
    {
      id: 'proxy',
      label: 'Proxy',
      disabled: disableProxy,
      activeTextClass: 'text-[#4ADE80] font-semibold'
    },
    {
      id: 'tun',
      label: 'TUN',
      activeTextClass: 'text-[#60A5FA] font-semibold'
    }
  ]

  return (
    <div
      data-guide="home-power-toggle"
      className={cn(
        'w-full grid grid-cols-3 p-1 rounded-[14px] bg-[#262626]/75 dark:bg-[#262626]/75 backdrop-blur-md border border-white/10 shadow-lg select-none',
        className
      )}
    >
      {segments.map((seg) => {
        const selected = mode === seg.id
        const isDisabled = loading || Boolean(seg.disabled)
        return (
          <button
            key={seg.id}
            type="button"
            disabled={isDisabled}
            onClick={() => onChange(seg.id)}
            className={cn(
              'h-9 px-2 rounded-[11px] inline-flex items-center justify-center gap-1.5 text-[13px] transition-all duration-150 cursor-pointer disabled:opacity-45 disabled:cursor-default',
              selected
                ? cn('bg-background shadow-sm', seg.activeTextClass)
                : 'text-muted-foreground hover:text-foreground hover:bg-accent/50'
            )}
          >
            {loading && selected && <Spinner className="size-3" />}
            <span className="truncate">{seg.label}</span>
          </button>
        )
      })}
    </div>
  )
}

export default OffProxyTunSelector
