// The tree of views the render layer draws, kept in the logic layer as
// plain objects, and the `setData` paths that bring the render layer up to
// date after each batch of operations (`crates/nsh-platform-miniprogram/
// src/encode.rs` lists them).
//
// A node is `{ id, k, f, s, p, io, t, h, cs, im, c }`: its kind, frame
// `[x, y, width, height]`, inline CSS, whether it takes presses (`p`) and
// press in / out (`io`), its text, a scroll view's axis and content size,
// an image, and its children in drawing order (the last on top). The page's
// data is `{ root }`, the node of NSH's root view.

'use strict'

/** NSH's root view (`nsh_core::ROOT_ID`). */
const ROOT_ID = 1

/** The bits of op `e` (`encode::events`). */
const PRESS = 1
const PRESS_IN_OUT = 2
const LONG_PRESS = 4

function node(id, kind) {
  return { id, k: kind, f: [0, 0, 0, 0], s: '', p: false, io: false, lp: false, t: null, h: false, cs: null, im: null, c: [] }
}

class Tree {
  constructor() {
    this.root = node(ROOT_ID, 'view')
    this.nodes = new Map([[ROOT_ID, this.root]])
    // Kept apart from the nodes, which `setData` copies whole.
    this.parents = new Map()
  }

  /**
   * Applies one batch's operations; returns the `setData` object that
   * updates the render layer (empty when nothing it shows changed).
   */
  apply(ops) {
    // Nodes whose children changed, and nodes whose own fields changed
    // (field names by id).
    const structure = new Set()
    const fields = new Map()
    const touch = (id, field) => {
      if (!fields.has(id)) fields.set(id, new Set())
      fields.get(id).add(field)
    }
    for (const op of ops) {
      const id = op[1]
      switch (op[0]) {
        case 'c':
          this.nodes.set(id, node(id, op[2]))
          break
        case 'i': {
          const parent = this.nodes.get(id)
          const child = this.nodes.get(op[2])
          if (!parent || !child) break
          this.detach(child, structure)
          const before = op[3] == null ? -1 : parent.c.findIndex((n) => n.id === op[3])
          if (before < 0) parent.c.push(child)
          else parent.c.splice(before, 0, child)
          this.parents.set(child.id, parent.id)
          structure.add(parent.id)
          break
        }
        case 'r': {
          const child = this.nodes.get(id)
          if (child) this.detach(child, structure)
          break
        }
        case 'd': {
          const child = this.nodes.get(id)
          if (child) this.detach(child, structure)
          this.nodes.delete(id)
          fields.delete(id)
          break
        }
        case 'f':
          this.set(id, 'f', [op[2], op[3], op[4], op[5]], touch)
          break
        case 's':
          this.set(id, 's', op[2], touch)
          break
        case 't':
          this.set(id, 't', op[2], touch)
          break
        case 'e': {
          const bits = op[2]
          this.set(id, 'p', (bits & PRESS) !== 0, touch)
          this.set(id, 'io', (bits & PRESS_IN_OUT) !== 0, touch)
          this.set(id, 'lp', (bits & LONG_PRESS) !== 0, touch)
          break
        }
        case 'sc':
          this.set(id, 'h', op[2], touch)
          break
        case 'ss':
          this.set(id, 'cs', [op[2], op[3]], touch)
          break
        case 'im':
          this.set(id, 'im', op[2] == null ? null : { src: op[2], mode: op[3] }, touch)
          break
        default:
          break
      }
    }
    return this.paths(structure, fields)
  }

  set(id, field, value, touch) {
    const n = this.nodes.get(id)
    if (!n) return
    n[field] = value
    touch(id, field)
  }

  detach(child, structure) {
    const parentId = this.parents.get(child.id)
    if (parentId == null) return
    const parent = this.nodes.get(parentId)
    if (parent) {
      const i = parent.c.indexOf(child)
      if (i >= 0) parent.c.splice(i, 1)
      structure.add(parent.id)
    }
    this.parents.delete(child.id)
  }

  /** The data path of node `id` (`root.c[2].c[0]`), or null when detached. */
  path(id) {
    const steps = []
    let current = id
    while (current !== ROOT_ID) {
      const parentId = this.parents.get(current)
      if (parentId == null) return null
      const parent = this.nodes.get(parentId)
      const i = parent.c.findIndex((n) => n.id === current)
      if (i < 0) return null
      steps.push(i)
      current = parentId
    }
    return 'root' + steps.reverse().map((i) => `.c[${i}]`).join('')
  }

  /** Whether an ancestor of `id` (or `id` itself, `self`) sends its children whole. */
  covered(id, structure, self) {
    let current = self ? id : this.parents.get(id)
    while (current != null) {
      if (structure.has(current)) return true
      current = this.parents.get(current)
    }
    return false
  }

  paths(structure, fields) {
    const data = {}
    for (const id of structure) {
      if (this.covered(id, structure, false)) continue
      const path = this.path(id)
      if (path != null) data[path + '.c'] = this.nodes.get(id).c
    }
    for (const [id, names] of fields) {
      // Sent with a parent's children (`covered`): the node object is the
      // same one, already up to date.
      if (this.covered(id, structure, false)) continue
      const path = this.path(id)
      if (path == null) continue
      const n = this.nodes.get(id)
      for (const name of names) data[`${path}.${name}`] = n[name]
    }
    return data
  }
}

module.exports = { Tree, ROOT_ID }
