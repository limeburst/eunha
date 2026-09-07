import * as React from "react"

// Matches the `md` breakpoint where the desktop rail (`.sidebar-frame`,
// `md:flex`) takes over. Below it, the Sidebar renders as a drawer.
const MOBILE_BREAKPOINT = 768

// Answered on the first render rather than in an effect afterwards. The
// generated hook started `undefined` and so said "not mobile" for one paint,
// which is invisible for a drawer that is closed anyway — but the home page
// now picks its whole layout from this, and a first answer of `false` on a
// phone would mount the advanced layout, three live feeds and all, only to
// throw it away a frame later.
export function useIsMobile() {
  const [isMobile, setIsMobile] = React.useState(
    () => window.innerWidth < MOBILE_BREAKPOINT,
  )

  React.useEffect(() => {
    const mql = window.matchMedia(`(max-width: ${MOBILE_BREAKPOINT - 1}px)`)
    const onChange = () => {
      setIsMobile(window.innerWidth < MOBILE_BREAKPOINT)
    }
    mql.addEventListener("change", onChange)
    onChange()
    return () => mql.removeEventListener("change", onChange)
  }, [])

  return isMobile
}
