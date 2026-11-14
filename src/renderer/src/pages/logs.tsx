import BasePage from '@renderer/components/base/base-page'
import LogItem from '@renderer/components/logs/log-item'
import ConnectionsEmpty from '@renderer/components/connections/connections-empty'
import { useCallback, useMemo, useRef, useState } from 'react'
import { Button } from '@renderer/components/ui/button'
import { Separator } from '@renderer/components/ui/separator'
import { Input } from '@renderer/components/ui/input'
import { Virtuoso, VirtuosoHandle } from 'react-virtuoso'
import { useTranslation } from 'react-i18next'

import { LogEntry, useLogsStore } from '@renderer/store/logs-store'
import { includesIgnoreCase } from '@renderer/utils/includes'
import { ArrowUp, Pause, Play, ScrollText, SearchX, Trash2 } from 'lucide-react'

// How far from the top still counts as "watching the latest lines".
const AT_TOP_THRESHOLD = 48

const matchesFilter = (log: LogEntry, filter: string): boolean =>
  includesIgnoreCase(log.payload, filter) || includesIgnoreCase(log.type, filter)

const computeItemKey = (_: number, log: LogEntry): number => log.id

const renderItem = (_: number, log: LogEntry): React.ReactNode => (
  <LogItem seq={log.id} type={log.type} payload={log.payload} time={log.time} clock={log.clock} />
)

const listComponents = { Header: () => <div className="h-1" /> }

const Logs: React.FC = () => {
  const { t } = useTranslation()
  const storeLogs = useLogsStore((s) => s.logs)
  const clearLogs = useLogsStore((s) => s.clear)
  const [filter, setFilter] = useState('')
  const [selectedLevel, setSelectedLevel] = useState<'all' | 'error' | 'warning' | 'info' | 'tun'>('all')
  const [paused, setPaused] = useState(false)
  const [atTop, setAtTop] = useState(true)
  const virtuosoRef = useRef<VirtuosoHandle>(null)

  // New lines are prepended, so the list only follows the store while it is scrolled to the
  // top and not paused. Otherwise it keeps the last snapshot and doesn't shift under the reader.
  const live = !paused && atTop
  const [shownLogs, setShownLogs] = useState(storeLogs)
  if (live && shownLogs !== storeLogs) {
    setShownLogs(storeLogs)
  }

  const filteredLogs = useMemo(() => {
    return shownLogs.filter((log) => {
      if (selectedLevel === 'tun') {
        const payloadLow = log.payload.toLowerCase()
        if (!payloadLow.includes('tun') && !payloadLow.includes('wintun')) return false
      } else if (selectedLevel !== 'all') {
        if (log.type.toLowerCase() !== selectedLevel) return false
      }
      if (filter !== '') {
        return matchesFilter(log, filter)
      }
      return true
    })
  }, [shownLogs, filter, selectedLevel])

  const newCount = useMemo(() => {
    const shownTopId = shownLogs[0]?.id ?? 0
    let count = 0
    for (const log of storeLogs) {
      if (log.id <= shownTopId) break
      if (selectedLevel === 'tun') {
        const payloadLow = log.payload.toLowerCase()
        if (!payloadLow.includes('tun') && !payloadLow.includes('wintun')) continue
      } else if (selectedLevel !== 'all') {
        if (log.type.toLowerCase() !== selectedLevel) continue
      }
      if (filter === '' || matchesFilter(log, filter)) count++
    }
    return count
  }, [storeLogs, shownLogs, filter, selectedLevel])

  const showLatest = useCallback(() => {
    setPaused(false)
    virtuosoRef.current?.scrollTo({ top: 0 })
  }, [])

  const handleClear = useCallback(() => {
    clearLogs()
    setShownLogs([])
  }, [clearLogs])

  const emptyState =
    filteredLogs.length > 0 ? null : filter !== '' ? (
      <ConnectionsEmpty
        icon={SearchX}
        title={t('pages.logs.emptyFilterTitle')}
        description={t('pages.logs.emptyFilterDescription')}
        action={{ label: t('pages.logs.clearFilter'), onClick: () => setFilter('') }}
      />
    ) : (
      <ConnectionsEmpty
        icon={ScrollText}
        title={t('pages.logs.emptyTitle')}
        description={t('pages.logs.emptyDescription')}
      />
    )

  const pauseLabel = paused ? t('pages.logs.resume') : t('pages.logs.pause')
  // The newest line has the largest number, so it sets the width of the number column.
  const seqWidth = `${String(shownLogs[0]?.id ?? 1).length}ch`

  return (
    <BasePage title={t('pages.logs.title')}>
      <div className="sticky top-0 z-40">
        <div className="w-full flex px-2 pb-2">
          <Input
            className="h-8 text-sm"
            value={filter}
            placeholder={t('common.filter')}
            onChange={(e) => setFilter(e.target.value)}
          />
          <Button
            size="icon-sm"
            className="ml-2 p-0 bg-clip-border"
            variant={paused ? 'default' : 'outline'}
            title={pauseLabel}
            aria-label={pauseLabel}
            aria-pressed={paused}
            onClick={paused ? showLatest : () => setPaused(true)}
          >
            {paused ? <Play className="text-lg" /> : <Pause className="text-lg" />}
          </Button>
          <Button
            size="icon-sm"
            title={t('pages.logs.clearLogs')}
            aria-label={t('pages.logs.clearLogs')}
            className="ml-2 p-0 bg-clip-border"
            variant="ghost"
            onClick={handleClear}
          >
            <Trash2 className="text-lg text-destructive" />
          </Button>
        </div>
        <div className="w-full flex items-center gap-1.5 px-2 pb-2">
          {(
            [
              ['all', 'ALL'],
              ['error', 'ERROR'],
              ['warning', 'WARN'],
              ['info', 'INFO'],
              ['tun', 'TUN']
            ] as const
          ).map(([lvl, label]) => (
            <Button
              key={lvl}
              size="sm"
              variant={selectedLevel === lvl ? 'default' : 'outline'}
              className="h-6 text-xs px-2.5 rounded-full"
              onClick={() => setSelectedLevel(lvl)}
            >
              {label}
            </Button>
          ))}
        </div>
        <Separator className="mx-2" />
      </div>
      <div className="relative h-[calc(100vh-142px)] mt-px">
        {/* Stays mounted when empty so its at-top state never goes stale. */}
        <Virtuoso
          ref={virtuosoRef}
          data={filteredLogs}
          initialItemCount={Math.min(filteredLogs.length, 15)}
          atTopThreshold={AT_TOP_THRESHOLD}
          atTopStateChange={setAtTop}
          computeItemKey={computeItemKey}
          components={listComponents}
          style={{ '--log-seq-width': seqWidth } as React.CSSProperties}
          itemContent={renderItem}
        />
        {emptyState && <div className="absolute inset-0">{emptyState}</div>}
        {newCount > 0 && (
          <Button
            size="sm"
            className="absolute top-3 left-1/2 z-10 -translate-x-1/2 rounded-full shadow-md"
            onClick={showLatest}
          >
            <ArrowUp />
            {t('pages.logs.newLogs', { count: newCount })}
          </Button>
        )}
      </div>
    </BasePage>
  )
}

export default Logs
