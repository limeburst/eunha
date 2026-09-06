import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react'
import { Plus, X } from 'lucide-react'

import { getToken } from '../auth.ts'
import { cn } from '@/lib/utils.ts'
import { PANES, paneTitle, readPanes, writePanes, type PaneId } from '../lib/panes.ts'
import { TopBar } from '@/components/top-bar.tsx'
import { ColumnHeader } from '@/components/column-header.tsx'
import { StatusFeed } from '@/components/status-feed.tsx'
import { NotificationsFeed } from '@/pages/Notifications.tsx'
import { MessagesFeed } from '@/pages/Messages.tsx'
import { useComposeModal } from '@/components/compose-modal.tsx'
import { Button } from '@/components/ui/button.tsx'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu.tsx'

/**
 * Several timelines at once, for people who want them.
 *
 * Each pane scrolls on its own and holds its own stream, which is the point —
 * the single column can only show one feed and makes you navigate to compare.
 * The row scrolls sideways when the panes outgrow the window rather than
 * squeezing them, because a timeline narrower than its posts is not a timeline.
 */
// Each pane is one of three shapes: a status timeline, the notification list,
// or the message list. They keep their own scroll and their own stream, which
// is the point of showing them at once.
function PaneBody({
  id,
  token,
  openCompose,
}: {
  id: PaneId
  token: string | null
  openCompose: ReturnType<typeof useComposeModal>['openCompose']
}) {
  if (id === 'notifications') return <NotificationsFeed />
  if (id === 'messages') return <MessagesFeed />
  return (
    <StatusFeed
      kind={id}
      token={token}
      onReply={(status, prepend) =>
        openCompose({ replyTo: status, onPosted: prepend })
      }
    />
  )
}

// Kept in step with `.advanced-frame` and `.sidebar-frame` in styles.css.
const RAIL_REM = 14
const GAP_REM = 0.75

function remToPx(rem: number): number {
  return rem * parseFloat(getComputedStyle(document.documentElement).fontSize)
}

export function AdvancedLayout() {
  const token = getToken()
  const { openCompose } = useComposeModal()
  const [panes, setPanes] = useState<PaneId[]>(() => readPanes())

  const frameRef = useRef<HTMLDivElement>(null)
  // The panes' own elements: what is drawn under the cursor while one of them
  // is dragged, and what both the slide and the drop are measured from.
  const paneNodes = useRef(new Map<PaneId, HTMLDivElement>())
  // Where each pane sat at the last render, for the slide between orders.
  const lefts = useRef(new Map<PaneId, number>())

  // Tells the stylesheet this layout is mounted: the rail's default `left`
  // assumes a centred reading column, which this does not have.
  useEffect(() => {
    document.documentElement.dataset.layout = 'advanced'
    return () => {
      delete document.documentElement.dataset.layout
    }
  }, [])

  // Where the rail and the panes, taken together, should start.
  //
  // The rail is fixed, so it cannot be centred by the flow it is not in — and
  // the panes cannot be centred alone or the group would sit off to one side
  // of its own rail. Both are placed from one number: the group's left edge,
  // which is the middle when it fits and a pinned margin when it does not.
  // Measured rather than computed from pane widths, because the row also
  // carries the Add button and its padding, and a formula that forgot either
  // would drift.
  const place = useCallback(() => {
    const row = frameRef.current
    if (!row) return
    // Summed from the children's own widths rather than from where they land.
    // Measuring a position would read back the very `margin-left` this sets,
    // and observing the row for it would be a resize loop the browser aborts
    // without saying so. A pane's width does not depend on where the row
    // starts, so this is both stable and cheap.
    const style = getComputedStyle(row)
    const gap = parseFloat(style.columnGap) || 0
    const padding =
      (parseFloat(style.paddingLeft) || 0) + (parseFloat(style.paddingRight) || 0)
    const kids = Array.from(row.children) as HTMLElement[]
    if (kids.length === 0) return
    const content =
      kids.reduce((sum, kid) => sum + kid.offsetWidth, 0) +
      gap * (kids.length - 1) +
      padding

    const group = remToPx(RAIL_REM + GAP_REM) + content
    const left = Math.max(remToPx(1), Math.round((window.innerWidth - group) / 2))
    document.documentElement.style.setProperty('--adv-left', `${left}px`)
  }, [])

  // `useLayoutEffect` so the first paint is already in the right place rather
  // than jumping once measured. The observer watches the viewport only — the
  // row is what this positions, so watching it too would feed back.
  useLayoutEffect(() => {
    place()
    const observer = new ResizeObserver(place)
    observer.observe(document.documentElement)
    window.addEventListener('resize', place)
    return () => {
      observer.disconnect()
      window.removeEventListener('resize', place)
      document.documentElement.style.removeProperty('--adv-left')
    }
  }, [place, panes.length])

  const update = (next: PaneId[]) => {
    setPanes(next)
    writePanes(next)
  }

  // Columns slide to the place the reorder gave them rather than appearing in
  // it. A pane that moves aside while another is dragged over it has to be
  // seen doing so, or the row simply differs from one frame to the next and
  // the reader is left to work out what changed.
  //
  // Measured after the layout effect above rather than before: closing a pane
  // moves every other one twice over — once for the gap it leaves and once
  // for the re-centring — and reading the position between the two would
  // animate from a place the panes were never in.
  useLayoutEffect(() => {
    const was = lefts.current
    const now = new Map<PaneId, number>()
    for (const [id, node] of paneNodes.current) {
      now.set(id, node.getBoundingClientRect().left)
    }
    lefts.current = now
    const still = window.matchMedia('(prefers-reduced-motion: reduce)').matches
    if (was.size === 0 || still) return
    for (const [id, node] of paneNodes.current) {
      const from = was.get(id)
      const to = now.get(id)
      if (from === undefined || to === undefined || from === to) continue
      // Put it back where it was without a transition, then take that away
      // with one: the browser animates the difference rather than the layout.
      node.style.transition = 'none'
      node.style.transform = `translateX(${from - to}px)`
      node.getBoundingClientRect()
      node.style.transition = ''
      node.style.transform = ''
    }
  }, [panes])

  // The last pane stays. Closing it would leave the layout with nothing in it
  // and no way back except the Add menu, which is a dead end rather than a
  // choice — and "advanced layout, showing nothing" is not a state worth being
  // able to reach.
  const canClose = panes.length > 1
  const remove = (id: PaneId) => {
    if (!canClose) return
    update(panes.filter((p) => p !== id))
  }
  const add = (id: PaneId) => update([...panes, id])
  const available = PANES.filter((p) => !panes.includes(p.id))

  // Which pane the pointer is carrying, if any. The order the row was in when
  // it was picked up is kept beside it — not to apply at the drop, which needs
  // nothing applied, but to put back if the drag is abandoned.
  const [dragging, setDragging] = useState<PaneId | null>(null)
  const before = useRef<PaneId[]>([])

  // Move a pane to where another one sits, closing the gap it leaves behind.
  const moveTo = (id: PaneId, to: number) => {
    const from = panes.indexOf(id)
    if (from < 0 || to < 0 || to >= panes.length || to === from) return
    const next = [...panes]
    next.splice(to, 0, ...next.splice(from, 1))
    update(next)
  }

  // Carrying a column out of the row and letting go there is how a drag is
  // called off: the order goes back to the one it was picked up from.
  const outsideRow = (at: { x: number; y: number }) => {
    const box = frameRef.current?.getBoundingClientRect()
    if (!box) return false
    return at.x < box.left || at.x > box.right || at.y < box.top || at.y > box.bottom
  }

  // Which place in the row the pointer is over — measured from the row rather
  // than read off whatever element is under the cursor.
  //
  // Asking what is under it is the obvious way and it flaps: a pane sliding
  // into its new place is *drawn* where it used to be for as long as the slide
  // lasts, so the pointer that just moved a column right is still over that
  // column's old neighbour, and the next `dragover` moves it back. A drag
  // creeping across one boundary was seen swapping and unswapping. The layout
  // is unmoved by any of that — every pane is one width, so which place the
  // pointer is in is arithmetic.
  const placeUnder = (clientX: number) => {
    const row = frameRef.current
    const pane = paneNodes.current.values().next().value
    if (!row || !pane) return -1
    const style = getComputedStyle(row)
    const step = pane.offsetWidth + (parseFloat(style.columnGap) || 0)
    const pad = parseFloat(style.paddingLeft) || 0
    const x = clientX - row.getBoundingClientRect().left + row.scrollLeft - pad
    return Math.max(0, Math.min(panes.length - 1, Math.floor(x / step)))
  }

  return (
    <>
      <TopBar />
      <div
        ref={frameRef}
        className="advanced-frame"
        // The row is the drop target, not each pane: what a place in it means
        // is the row's arithmetic, and the gaps and the Add button at the end
        // are as much a part of it as the columns are.
        onDragOver={(e) => {
          if (!dragging) return
          e.preventDefault()
          e.dataTransfer.dropEffect = 'move'
          moveTo(dragging, placeUnder(e.clientX))
        }}
        // Nothing to apply at the drop: the row is already in the order the
        // pointer left it in.
        onDrop={(e) => {
          e.preventDefault()
          setDragging(null)
        }}
      >
        {panes.map((id, i) => (
          <div
            key={id}
            ref={(node) => {
              if (node) paneNodes.current.set(id, node)
              else paneNodes.current.delete(id)
            }}
            className={cn('advanced-pane', dragging === id && 'opacity-40')}
          >
            <ColumnHeader
              title={paneTitle(id)}
              // A lone pane has no order to be in, so its bar is a plain
              // header rather than a grip that can only put it back.
              reorder={
                panes.length > 1
                  ? {
                      column: () => paneNodes.current.get(id) ?? null,
                      onDragStart: () => {
                        before.current = panes
                        setDragging(id)
                      },
                      onDragEnd: (at) => {
                        if (outsideRow(at)) update(before.current)
                        setDragging(null)
                      },
                      onMove: (by) => moveTo(id, i + by),
                    }
                  : undefined
              }
            >
              <Button
                variant="ghost"
                size="icon"
                aria-label={`Close ${paneTitle(id)}`}
                title={
                  canClose
                    ? `Close ${paneTitle(id)}`
                    : 'The last column cannot be closed'
                }
                disabled={!canClose}
                onClick={() => remove(id)}
              >
                <X />
              </Button>
            </ColumnHeader>
            <div className="flex-1 space-y-2 overflow-y-auto p-3">
              <PaneBody id={id} token={token} openCompose={openCompose} />
            </div>
          </div>
        ))}

        {available.length > 0 && (
          <div className="shrink-0 self-start pt-3">
            <DropdownMenu>
              <DropdownMenuTrigger
                render={<Button variant="outline" size="sm" />}
                aria-label="Add a timeline"
              >
                <Plus /> Add
              </DropdownMenuTrigger>
              <DropdownMenuContent align="start">
                {available.map((p) => (
                  <DropdownMenuItem key={p.id} onClick={() => add(p.id)}>
                    {p.title}
                  </DropdownMenuItem>
                ))}
              </DropdownMenuContent>
            </DropdownMenu>
          </div>
        )}
      </div>
    </>
  )
}
