import { useRef, type ReactNode } from 'react'

import { cn } from '@/lib/utils.ts'

/**
 * What it takes to make the bar a handle for reordering its column.
 *
 * Passed only by the advanced layout: a single column has no order to change,
 * and a header that offered to move it would be lying.
 */
export type ColumnReorder = {
  onDragStart: () => void
  /** Where the column was let go of, in client coordinates. */
  onDragEnd: (at: { x: number; y: number }) => void
  /** Move this column one place left (-1) or right (1). */
  onMove: (by: -1 | 1) => void
  /** The column this bar heads, which is what is drawn under the cursor. */
  column: () => HTMLElement | null
}

/**
 * The heading at the top of a column.
 *
 * Mastodon 5.0 standardised these — the announcement counts "at least 12
 * different headers" before it — and gives each column one bar with the same
 * shape: a title that scrolls the column back to the top when clicked, and
 * room on the right for whatever that page can do.
 *
 * The title is not always the navigation's word for the page. Home is labelled
 * "Following", because that is what the feed *is*; the rail already says where
 * you are.
 *
 * Given `reorder`, the whole bar is also the grip that drags its column into a
 * new place — the bar rather than a separate grip icon, because it is already
 * the part of a column that is not the column's contents, and a handle small
 * enough to need aiming at is a handle people miss.
 */
export function ColumnHeader({
  title,
  children,
  className,
  reorder,
}: {
  title: string
  /** Controls for this column, aligned right. */
  children?: ReactNode
  className?: string
  /** Present where the column can be moved; absent where it cannot. */
  reorder?: ColumnReorder
}) {
  const controls = useRef<HTMLDivElement>(null)

  return (
    <header
      draggable={reorder ? true : undefined}
      onDragStart={
        reorder &&
        ((e) => {
          // A press on the column's own controls is not a grip on the column.
          // `draggable={false}` on them does not say so — the browser looks
          // for the nearest draggable *ancestor* of what was pressed, and
          // finds this bar either way — so the press is placed by hand.
          const box = controls.current?.getBoundingClientRect()
          if (
            box &&
            e.clientX >= box.left &&
            e.clientX <= box.right &&
            e.clientY >= box.top &&
            e.clientY <= box.bottom
          ) {
            e.preventDefault()
            return
          }
          // Firefox starts no drag at all without something on the transfer,
          // even where — as here — nothing reads it back: the order is moved
          // as the pointer passes, not decided from a payload at the drop.
          e.dataTransfer.effectAllowed = 'move'
          e.dataTransfer.setData('text/plain', title)
          // Drag the column, not the bar. Left to itself the browser draws
          // the element the drag started on, and a 3rem strip of translucent
          // header under the cursor reads as though the title alone had come
          // away from the column it belongs to.
          const column = reorder.column()
          if (column) {
            const rect = column.getBoundingClientRect()
            e.dataTransfer.setDragImage(
              column,
              e.clientX - rect.left,
              e.clientY - rect.top,
            )
          }
          reorder.onDragStart()
        })
      }
      // Where it was let go of, for the layout to judge. `dropEffect` would
      // seem the thing to read — "none" means nothing took the drop — but a
      // browser that reports a real drop that way would undo a move the reader
      // had already watched happen, and losing a finished drag is worse than
      // keeping an abandoned one. Chromium does report it that way for a
      // synthetic drag released over a pane, which is how this was found.
      onDragEnd={reorder && ((e) => reorder.onDragEnd({ x: e.clientX, y: e.clientY }))}
      // The same move from the keyboard. Dragging is the discoverable way and
      // the only one a mouse offers, but it is also the one nobody can do
      // without a pointer, so the arrows do it from the bar's own controls.
      onKeyDown={
        reorder &&
        ((e) => {
          if (e.key !== 'ArrowLeft' && e.key !== 'ArrowRight') return
          e.preventDefault()
          reorder.onMove(e.key === 'ArrowLeft' ? -1 : 1)
        })
      }
      className={cn(
        'bg-card/85 sticky top-0 z-30 flex items-center gap-2 rounded-t-lg border-b px-3 py-2 backdrop-blur',
        reorder && 'cursor-grab active:cursor-grabbing',
        className,
      )}
    >
      <button
        type="button"
        className="min-w-0 flex-1 truncate text-left text-sm font-semibold"
        title={reorder ? 'Drag to reorder, or press ← and →' : undefined}
        onClick={() => window.scrollTo({ top: 0, behavior: 'smooth' })}
      >
        {title}
      </button>
      {children && (
        // Not a place to pick the column up from: these are the things a
        // column can do, and closing one is not a way to start moving it.
        <div ref={controls} className="flex shrink-0 items-center gap-1">
          {children}
        </div>
      )}
    </header>
  )
}
