'use strict'

// `node --test runtime/test/`: the tree the render layer draws and the
// runtime against a stand-in for the Mini Program's APIs.

const test = require('node:test')
const assert = require('node:assert/strict')
const { Tree, ROOT_ID } = require('../nsh/tree.js')

test('inserts build the tree and send the root children whole', () => {
  const tree = new Tree()
  const data = tree.apply([
    ['c', 2, 'view'],
    ['c', 4, 'text'],
    ['i', 2, 4, null],
    ['i', ROOT_ID, 2, null],
    ['f', 2, 0, 0, 100, 40],
    ['t', 4, { lines: [['Hi', 0, 0]] }],
  ])
  assert.deepEqual(Object.keys(data), ['root.c'])
  assert.equal(data['root.c'][0].id, 2)
  assert.deepEqual(data['root.c'][0].f, [0, 0, 100, 40])
  assert.equal(data['root.c'][0].c[0].t.lines[0][0], 'Hi')
})

test('later changes are sent by path', () => {
  const tree = new Tree()
  tree.apply([
    ['c', 2, 'view'],
    ['c', 4, 'view'],
    ['c', 6, 'text'],
    ['i', ROOT_ID, 2, null],
    ['i', 2, 4, null],
    ['i', 2, 6, null],
  ])
  const data = tree.apply([
    ['t', 6, { lines: [['Count 1', 0, 0]] }],
    ['e', 4, 1],
  ])
  assert.deepEqual(data, {
    'root.c[0].c[1].t': { lines: [['Count 1', 0, 0]] },
    'root.c[0].c[0].p': true,
    'root.c[0].c[0].io': false,
    'root.c[0].c[0].lp': false,
  })
})

test('insert below a sibling, move, remove and delete', () => {
  const tree = new Tree()
  tree.apply([
    ['c', 2, 'view'],
    ['c', 4, 'view'],
    ['c', 6, 'view'],
    ['i', ROOT_ID, 2, null],
    ['i', ROOT_ID, 4, null],
    ['i', ROOT_ID, 6, 4],
  ])
  assert.deepEqual(tree.root.c.map((n) => n.id), [2, 6, 4])
  // A move: the child leaves its place.
  tree.apply([['i', ROOT_ID, 2, null]])
  assert.deepEqual(tree.root.c.map((n) => n.id), [6, 4, 2])
  const data = tree.apply([['r', 6], ['d', 4]])
  assert.deepEqual(data['root.c'].map((n) => n.id), [2])
  assert.equal(tree.path(6), null)
  // A detached node's changes are kept for when it is inserted again.
  assert.deepEqual(tree.apply([['s', 6, 'opacity:0.5;']]), {})
  const back = tree.apply([['i', 2, 6, null]])
  assert.equal(back['root.c[0].c'][0].s, 'opacity:0.5;')
})

test('a parent sent whole covers its descendants changes', () => {
  const tree = new Tree()
  tree.apply([
    ['c', 2, 'view'],
    ['c', 4, 'view'],
    ['i', ROOT_ID, 2, null],
    ['i', 2, 4, null],
  ])
  const data = tree.apply([
    ['c', 6, 'view'],
    ['i', 2, 6, null],
    ['s', 4, 'background-color:rgb(0,0,0);'],
  ])
  assert.deepEqual(Object.keys(data), ['root.c[0].c'])
})
