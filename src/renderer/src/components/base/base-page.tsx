import React from 'react'
import { useNavigate } from 'react-router-dom'
import { ArrowLeft } from 'lucide-react'
import { Button } from '@renderer/components/ui/button'
import { cn } from '@renderer/lib/utils'

interface BasePageProps {
  title?: React.ReactNode
  header?: React.ReactNode
  children?: React.ReactNode
  className?: string
  showBackButton?: boolean
}

const BasePage = React.forwardRef<HTMLDivElement, BasePageProps>((props, ref) => {
  const { title, header, children, className, showBackButton = false } = props
  const navigate = useNavigate()
  const hasHeader = Boolean(title || header || showBackButton)

  if (!hasHeader) {
    return (
      <div ref={ref} className={cn('w-full h-full overflow-hidden', className)}>
        {children}
      </div>
    )
  }

  return (
    <div ref={ref} className={cn('w-full h-full flex flex-col overflow-hidden', className)}>
      <div className="sticky top-0 z-40 h-11 shrink-0 border-b border-border bg-background/95 backdrop-blur-md flex items-center justify-between px-4">
        <div className="flex items-center gap-2 min-w-0">
          {showBackButton && (
            <Button
              size="icon-sm"
              variant="ghost"
              className="app-nodrag cursor-pointer"
              onClick={() => navigate(-1)}
            >
              <ArrowLeft className="size-4" />
            </Button>
          )}
          {title && (
            <span className="text-sm font-semibold tracking-tight text-foreground truncate">
              {title}
            </span>
          )}
        </div>
        {header && <div className="flex items-center gap-1.5 shrink-0">{header}</div>}
      </div>
      <div className="content flex-1 min-h-0 w-full overflow-y-auto px-2 pt-2 pb-4">{children}</div>
    </div>
  )
})

BasePage.displayName = 'BasePage'

export default BasePage
