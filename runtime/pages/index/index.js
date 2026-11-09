// The page the NSH app runs in: `nsh/nsh-mp.js` keeps `root`, NSH's root
// view, up to date; taps and scrolls of the views come back as `nsh` events
// of the `nsh-node` components.
const { start } = require('../../nsh/nsh-mp.js')

Page({
  data: { root: { id: 1, c: [] } },

  onLoad() {
    start(this)
      .then((app) => {
        this.app = app
      })
      .catch((error) => console.error('nsh: the app did not start', error))
  },

  onUnload() {
    if (this.app) this.app.stop()
  },

  onNsh(e) {
    if (!this.app) return
    const { kind, id, x, y } = e.detail
    if (kind === 'scroll') this.app.scroll(id, x, y)
    else this.app.event(id, kind)
  },
})
