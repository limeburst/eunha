import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react'
import { Plus, X } from 'lucide-react'

import { getToken } from '../auth.ts'
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

  // Reordering happens as the pointer passes rather than at the drop: the pane
  // being dragged takes the place of whichever one it is over, so the row
  // always shows the order that letting go would leave. That needs no
  // insertion line to read, and it cannot oscillate — every pane is one width,
  // so a swap leaves the dragged pane, not a boundary, back under the pointer.
  const dragOver = (over: PaneId) => {
    if (!dragging || over === dragging) return
    moveTo(dragging, panes.indexOf(over))
  }

  return (
    <>
      <TopBar />
      <div ref={frameRef} className="advanced-frame">
        {panes.map((id, i) => (
          <div
            key={id}
            className="advanced-pane"
            // The whole pane is the target, not just its header: aiming at a
            // 2rem bar to say "here" is harder than aiming at the column it
            // belongs to, and the column is what is being placed.
            onDragOver={(e) => {
              if (!dragging) return
              e.preventDefault()
              e.dataTransfer.dropEffect = 'move'
              dragOver(id)
            }}
            // Nothing to apply at the drop: the row is already in the order
            // the pointer left it in. Settling it once more against whatever
            // is under the cursor was tried and is wrong — the pane the last
            // `dragover` named has by then been pushed along by that very
            // swap, so re-reading it moves the dragged pane one place too far.
            onDrop={(e) => {
              e.preventDefault()
              setDragging(null)
            }}
          >
            <ColumnHeader
              title={paneTitle(id)}
              // A lone pane has no order to be in, so its bar is a plain
              // header rather than a grip that can only put it back.
              reorder={
                panes.length > 1
                  ? {
                      dragging: dragging === id,
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
