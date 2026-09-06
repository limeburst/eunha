import { expect, test } from '@playwright/test'

const me = {
  id: '1',
  username: 'alice',
  acct: 'alice',
  display_name: 'Alice',
  avatar: '',
  source: { privacy: 'public' },
}

async function signedIn(page: import('@playwright/test').Page) {
  await page.addInitScript(() => localStorage.setItem('eunha:token', 'test-token'))
  await page.route('**/api/v1/accounts/verify_credentials**', (r) => r.fulfill({ json: me }))
  await page.route('**/api/v1/notifications/unread_count**', (r) =>
    r.fulfill({ json: { count: 0 } }),
  )
  await page.route('**/api/v1/timelines/**', (r) => r.fulfill({ json: [] }))
  await page.route('**/api/v1/notifications**', (r) => r.fulfill({ json: [] }))
  await page.route('**/api/v1/conversations**', (r) => r.fulfill({ json: [] }))
}

// The nav says Home; the column says what the feed is. Both are deliberate,
// and the accessible label stays "Home" so the two never disagree for a screen
// reader.
test('the home column is headed Following', async ({ page }) => {
  await signedIn(page)
  await page.goto('/')

  const column = page.locator('.column-frame')
  await expect(column.getByRole('button', { name: 'Following' })).toBeVisible()
  await expect(page.getByRole('region', { name: 'Home' })).toBeVisible()
  // And the rail still calls it Home.
  await expect(page.locator('aside').getByRole('link', { name: 'Home' })).toBeVisible()
})

test('local and federated columns are headed by their feed', async ({ page }) => {
  await signedIn(page)
  await page.goto('/local')
  await expect(
    page.locator('.column-frame').getByRole('button', { name: 'Local' }),
  ).toBeVisible()

  await page.goto('/public')
  await expect(
    page.locator('.column-frame').getByRole('button', { name: 'Federated' }),
  ).toBeVisible()
})

// The advanced layout is off unless asked for, which is the whole point of
// gating it — the default stays one column.
test('the advanced layout is opt-in and remembers its panes', async ({ page }) => {
  await signedIn(page)
  await page.goto('/')
  await expect(page.locator('.advanced-pane')).toHaveCount(0)

  await page.goto('/settings')
  await page.getByRole('switch').first().click()

  await page.goto('/')
  await expect(page.locator('.advanced-pane')).toHaveCount(3)

  // Closing one sticks across a reload, because it is stored, not just state.
  await page.getByRole('button', { name: 'Close Local' }).click()
  await expect(page.locator('.advanced-pane')).toHaveCount(2)
  await page.reload()
  await expect(page.locator('.advanced-pane')).toHaveCount(2)

  // Only what is missing is offered back.
  await page.getByRole('button', { name: 'Add a timeline' }).click()
  await expect(page.getByRole('menuitem')).toContainText(['Local'])
})

// It opens on the two panes a second column is *for* — the feed and what is
// addressed to you — plus one more to choose.
test('the advanced layout opens on timeline, notifications and one more', async ({
  page,
}) => {
  await signedIn(page)
  await page.goto('/settings')
  await page.getByRole('switch').first().click()
  await page.goto('/')

  const titles = page.locator('.advanced-pane > header button').first()
  await expect(titles).toHaveText('Following')
  await expect(page.locator('.advanced-pane')).toHaveCount(3)
  await expect(
    page
      .locator('.advanced-pane')
      .nth(1)
      .getByRole('button', { name: 'Notifications', exact: true }),
  ).toBeVisible()
})

// The rail's default `left` follows a centred reading column, which this
// layout does not have — unpinned, it lands on top of the first pane on any
// wide screen. Checked at a width where the old positioning overlapped by
// 200px.
test('the rail does not overlap the first pane on a wide screen', async ({ page }) => {
  await signedIn(page)
  await page.goto('/settings')
  await page.getByRole('switch').first().click()

  for (const width of [1280, 1600, 2200]) {
    await page.setViewportSize({ width, height: 900 })
    await page.goto('/')
    const rail = await page.locator('aside.sidebar-frame').boundingBox()
    const panes = await page.locator('.advanced-frame').boundingBox()
    expect(rail, `rail missing at ${width}`).not.toBeNull()
    expect(panes, `panes missing at ${width}`).not.toBeNull()
    expect(rail!.x + rail!.width, `overlap at ${width}px`).toBeLessThanOrEqual(panes!.x)
  }
})

// The rail is fixed, so it cannot be centred by a flow it is not in. Rail and
// panes are placed together from one measured number: centred while the group
// fits, pinned once it does not.
test('the rail and panes are centred together while they fit', async ({ page }) => {
  await signedIn(page)
  await page.goto('/settings')
  await page.getByRole('switch').first().click()

  const edges = async () => {
    const rail = await page.locator('aside.sidebar-frame').boundingBox()
    const gap = await page.evaluate(() => {
      const row = document.querySelector('.advanced-frame')
      if (!row) return null
      const style = getComputedStyle(row)
      const pad =
        (parseFloat(style.paddingLeft) || 0) + (parseFloat(style.paddingRight) || 0)
      const kids = Array.from(row.children) as HTMLElement[]
      const content =
        kids.reduce((sum, k) => sum + k.offsetWidth, 0) +
        (parseFloat(style.columnGap) || 0) * (kids.length - 1) +
        pad
      const left = row.getBoundingClientRect().left
      return window.innerWidth - (left + content)
    })
    return { left: rail!.x, right: gap! }
  }

  // Wide enough for rail plus three panes: even margins either side.
  await page.setViewportSize({ width: 2000, height: 900 })
  await page.goto('/')
  let e = await edges()
  expect(Math.abs(e.left - e.right), 'not centred at 2000px').toBeLessThan(4)
  expect(e.left, 'centred group should not be pinned').toBeGreaterThan(20)

  // Too narrow for the group: pinned left, and the panes scroll instead.
  await page.setViewportSize({ width: 1300, height: 900 })
  await page.goto('/')
  e = await edges()
  expect(Math.round(e.left), 'should pin once it stops fitting').toBe(16)

  // And it follows a resize, not just a fresh load.
  await page.setViewportSize({ width: 2000, height: 900 })
  await expect
    .poll(async () => {
      const after = await edges()
      return Math.abs(after.left - after.right) < 4
    })
    .toBe(true)
})

// A layout showing nothing is not a state worth being able to reach, so the
// last pane's close button goes dead rather than emptying the frame.
test('the last pane cannot be closed', async ({ page }) => {
  await signedIn(page)
  await page.goto('/settings')
  await page.getByRole('switch').first().click()
  await page.goto('/')

  await page.getByRole('button', { name: 'Close Local' }).click()
  await page.getByRole('button', { name: 'Close Notifications' }).click()
  await expect(page.locator('.advanced-pane')).toHaveCount(1)

  const close = page.getByRole('button', { name: 'Close Following' })
  await expect(close).toBeDisabled()
  await close.click({ force: true })
  await expect(page.locator('.advanced-pane')).toHaveCount(1)
})

// The order of the columns is the reader's, not ours. Dragging a header is
// the way a pointer says so; the arrows are the way anything else does.
test('a column can be dragged into a new place, and it sticks', async ({ page }) => {
  await signedIn(page)
  await page.goto('/settings')
  await page.getByRole('switch').first().click()
  await page.goto('/')

  const titles = page.locator('.advanced-pane > header > button')
  await expect(titles).toHaveText(['Following', 'Notifications', 'Local'])

  // Local, from the far end, onto Following at the near one.
  await page
    .locator('.advanced-pane')
    .nth(2)
    .locator('header')
    .dragTo(page.locator('.advanced-pane').nth(0))
  await expect(titles).toHaveText(['Local', 'Following', 'Notifications'])

  // Stored, not just state: an order that forgets itself on reload is not an
  // order anybody would arrange.
  await page.reload()
  await expect(titles).toHaveText(['Local', 'Following', 'Notifications'])

  // The far end in one pass and let go. The row reorders as the pointer
  // passes, and each pass costs a render, so a column crossing the whole row
  // in one movement is the case that would land short of where it was
  // dropped.
  const centre = async (n: number) => {
    const b = await page.locator('.advanced-pane').nth(n).boundingBox()
    return { x: b!.x + b!.width / 2, y: b!.y + 12 }
  }
  const from = await centre(0)
  const to = await centre(2)
  await page.mouse.move(from.x, from.y)
  await page.mouse.down()
  await page.mouse.move(to.x, to.y)
  await page.mouse.up()
  await expect(titles).toHaveText(['Following', 'Notifications', 'Local'])
})

// The bar is the grip, all of it: the title, the space either side of it, the
// padding above and below. Only the controls at the end are not, because
// pressing Close is not a way to start moving a column.
test('the whole bar is the grip, except the close button', async ({ page }) => {
  await signedIn(page)
  await page.goto('/settings')
  await page.getByRole('switch').first().click()
  await page.goto('/')

  const titles = page.locator('.advanced-pane > header > button')
  const header = page.locator('.advanced-pane > header').first()
  const box = (await header.boundingBox())!

  // Above the title, where the bar is padding and nothing else.
  const second = (await page.locator('.advanced-pane').nth(1).boundingBox())!
  await page.mouse.move(box.x + 40, box.y + 3)
  await page.mouse.down()
  await page.mouse.move(second.x + second.width / 2, box.y + 3)
  await page.mouse.up()
  await expect(titles).toHaveText(['Notifications', 'Following', 'Local'])

  // And the close button moves nothing — nor does the drag that was refused
  // leave a click behind that closes the column.
  const close = page.getByRole('button', { name: 'Close Notifications' })
  const cb = (await close.boundingBox())!
  await page.mouse.move(cb.x + cb.width / 2, cb.y + cb.height / 2)
  await page.mouse.down()
  await page.mouse.move(cb.x + 200, cb.y)
  await page.mouse.up()
  await expect(titles).toHaveText(['Notifications', 'Following', 'Local'])
  await expect(page.locator('.advanced-pane')).toHaveCount(3)
})

// A pane sliding into its new place is drawn where it used to be for as long
// as the slide lasts, so a drop decided by what is under the cursor puts the
// column back where it came from: creeping across one boundary was seen
// swapping and unswapping, ending where it started. The place is measured off
// the row instead, which the slide does not move.
test('creeping across a boundary settles on one order', async ({ page }) => {
  await signedIn(page)
  await page.goto('/settings')
  await page.getByRole('switch').first().click()
  await page.goto('/')

  const header = page.locator('.advanced-pane > header').first()
  const hb = (await header.boundingBox())!
  const second = (await page.locator('.advanced-pane').nth(1).boundingBox())!
  const read = async () =>
    (await page.locator('.advanced-pane > header > button').allTextContents()).join()

  const seen: string[] = []
  await page.mouse.move(hb.x + 40, hb.y + 3)
  await page.mouse.down()
  for (let x = hb.x + 60; x < second.x + second.width - 20; x += 12) {
    await page.mouse.move(x, hb.y + 3)
    const now = await read()
    if (seen[seen.length - 1] !== now) seen.push(now)
  }
  await page.mouse.up()

  expect(seen).toEqual(['Following,Notifications,Local', 'Notifications,Following,Local'])
  expect(await read()).toBe('Notifications,Following,Local')
})

// What follows the cursor is the column, not the bar it was picked up by.
test('the column is what is drawn under the cursor', async ({ page }) => {
  await signedIn(page)
  await page.addInitScript(() => {
    const real = DataTransfer.prototype.setDragImage
    ;(window as never as { __ghost: string[] }).__ghost = []
    DataTransfer.prototype.setDragImage = function (el, x, y) {
      ;(window as never as { __ghost: string[] }).__ghost.push(
        (el as HTMLElement).className,
      )
      return real.call(this, el, x, y)
    }
  })
  await page.goto('/settings')
  await page.getByRole('switch').first().click()
  await page.goto('/')

  await page
    .locator('.advanced-pane')
    .nth(0)
    .locator('header')
    .dragTo(page.locator('.advanced-pane').nth(1))

  const ghost = await page.evaluate(
    () => (window as never as { __ghost: string[] }).__ghost,
  )
  expect(ghost).toHaveLength(1)
  expect(ghost[0]).toContain('advanced-pane')
})

// A column that moves aside has to be seen doing it, or the row simply differs
// from one frame to the next and the reader has to work out what changed.
test('columns slide into their new places', async ({ page }) => {
  await signedIn(page)
  await page.goto('/settings')
  await page.getByRole('switch').first().click()
  await page.goto('/')

  await page.evaluate(() => {
    ;(window as never as { __slid: string[] }).__slid = []
    document.addEventListener(
      'transitionstart',
      (e) => {
        const t = e as TransitionEvent
        if (t.propertyName === 'transform') {
          ;(window as never as { __slid: string[] }).__slid.push(
            (t.target as HTMLElement).className,
          )
        }
      },
      true,
    )
  })

  const titles = page.locator('.advanced-pane > header > button')
  await titles.nth(0).focus()
  await page.keyboard.press('ArrowRight')
  await expect(titles).toHaveText(['Notifications', 'Following', 'Local'])

  // Both of the two that changed places, not just the one that was moved.
  const slid = await page.evaluate(
    () => (window as never as { __slid: string[] }).__slid,
  )
  expect(slid.filter((c) => c.includes('advanced-pane'))).toHaveLength(2)
})

// The row reorders under the pointer rather than at the drop, so a drag that
// is called off has already moved things. Letting go of nothing puts them back.
test('an abandoned drag leaves the order alone', async ({ page }) => {
  await signedIn(page)
  await page.goto('/settings')
  await page.getByRole('switch').first().click()
  await page.goto('/')

  const titles = page.locator('.advanced-pane > header > button')
  await expect(titles).toHaveText(['Following', 'Notifications', 'Local'])

  const pane = await page.locator('.advanced-pane').nth(0).boundingBox()
  const last = await page.locator('.advanced-pane').nth(2).boundingBox()
  await page.mouse.move(pane!.x + pane!.width / 2, pane!.y + 12)
  await page.mouse.down()
  // Out over the third column — the order moves — and then out of the row
  // entirely, where there is nothing to drop onto. Twice, because the first
  // move while held is what starts the drag; the second is what is dragged
  // *over* something.
  await page.mouse.move(last!.x + last!.width / 2, last!.y + 12)
  await page.mouse.move(last!.x + last!.width / 2, last!.y + 14)
  await expect(titles).toHaveText(['Notifications', 'Local', 'Following'])
  await page.mouse.move(last!.x + last!.width / 2, last!.y + last!.height + 40)
  await page.mouse.up()

  await expect(titles).toHaveText(['Following', 'Notifications', 'Local'])
  await page.reload()
  await expect(titles).toHaveText(['Following', 'Notifications', 'Local'])
})

// Dragging is the only move a mouse offers and the one nothing else can make,
// so the same move is on the arrow keys, from the header\'s own controls.
test('a column can be moved with the arrow keys', async ({ page }) => {
  await signedIn(page)
  await page.goto('/settings')
  await page.getByRole('switch').first().click()
  await page.goto('/')

  const titles = page.locator('.advanced-pane > header > button')
  await titles.nth(0).focus()
  await page.keyboard.press('ArrowRight')
  await expect(titles).toHaveText(['Notifications', 'Following', 'Local'])

  // Focus travels with the column rather than staying at the position, so a
  // second press carries on moving the same one.
  await page.keyboard.press('ArrowRight')
  await expect(titles).toHaveText(['Notifications', 'Local', 'Following'])

  // And it stops at the end rather than wrapping around to the front.
  await page.keyboard.press('ArrowRight')
  await expect(titles).toHaveText(['Notifications', 'Local', 'Following'])

  await page.keyboard.press('ArrowLeft')
  await expect(titles).toHaveText(['Notifications', 'Following', 'Local'])
})
