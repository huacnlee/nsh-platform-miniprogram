// One view of the NSH app at the frame the engine laid it out at
// (`node.f`), with its decoration (`node.s`), its text's lines (`node.t`)
// and its children: this component again (`index.json`). Taps on views
// that take presses and scrolls of scroll views go to the page as `nsh`
// events, which cross the components' boundaries (`composed`).
const LONG_PRESS = 'onLongPress'

Component({
  options: { addGlobalClass: false },
  properties: { node: Object },
  methods: {
    send(detail) {
      this.triggerEvent('nsh', detail, { bubbles: true, composed: true })
    },
    onTap() {
      this.send({ kind: 'onPress', id: this.data.node.id })
    },
    onLongPress() {
      this.send({ kind: LONG_PRESS, id: this.data.node.id })
    },
    onTouchStart() {
      this.send({ kind: 'onPressIn', id: this.data.node.id })
    },
    onTouchEnd() {
      this.send({ kind: 'onPressOut', id: this.data.node.id })
    },
    onScroll(e) {
      this.send({ kind: 'scroll', id: this.data.node.id, x: e.detail.scrollLeft, y: e.detail.scrollTop })
    },
    // A child's event passes on to the page.
    onChild(e) {
      this.send(e.detail)
    },
  },
})
