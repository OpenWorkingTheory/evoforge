// Checks the guided sequences in viewer/library.js.
//
//   node viewer/library_check.mjs
//
// These three functions decide what a viewer is shown when they ask for the
// best organism, for evolution as a flipbook, or for one organism's ancestry.
// Only a generation's top few and a random sample are recorded, so the
// interesting cases are all about absence: a parent that was never written, a
// founder with no parents at all, a generation nobody was recorded from. Each
// is easy to get wrong in a way that silently shows the wrong animal rather
// than failing, which is what makes them worth pinning down offline.

import { bestInRun, championsByGeneration, lineageOf } from './library.js';

let failures = 0;

function check(label, got, want) {
  const g = JSON.stringify(got);
  const w = JSON.stringify(want);
  if (g === w) {
    console.log(`  ok    ${label}`);
  } else {
    console.log(`  FAIL  ${label}\n          got  ${g}\n          want ${w}`);
    failures++;
  }
}

/** A replay header reduced to the fields the sequences actually read. */
const e = (id, generation, fitness, parents = [0, 0]) => ({ id, generation, fitness, parents });

const ids = (r) => r.items.map((x) => x.id);

// gen 0: two founders. gen 1: one child of 1. gen 2: a child of 3, plus a
// child of an organism (99) that was never recorded.
const run = [
  e(1, 0, 1.0),
  e(2, 0, 4.0),
  e(3, 1, 2.0, [1, 0]),
  e(4, 2, 9.0, [3, 0]),
  e(5, 2, 7.0, [99, 0]),
];

console.log('bestInRun');
check('picks the global maximum, not the last generation\'s', ids(bestInRun(run)), [4]);
check('empty run yields nothing', ids(bestInRun([])), []);

console.log('championsByGeneration');
check('one per generation, oldest first', ids(championsByGeneration(run)), [2, 3, 4]);
check('empty run yields nothing', ids(championsByGeneration([])), []);

console.log('lineageOf');
check('walks back to a founder, oldest first', ids(lineageOf(run, run[3])), [1, 3, 4]);
check('a founder is its own whole lineage', ids(lineageOf(run, run[0])), [1]);
check(
  'stops at an unrecorded ancestor rather than inventing a join',
  ids(lineageOf(run, run[4])),
  [5],
);
check('and reports which ancestor it could not reach', lineageOf(run, run[4]).missing, 99);
check('a complete chain reports no gap', lineageOf(run, run[3]).missing, null);

// A parent id pointing at a later organism would loop forever without the
// visited set. Nothing should produce this, which is exactly why it is cheap
// to be certain about.
const cyclic = [e(10, 1, 1.0, [11, 0]), e(11, 2, 2.0, [10, 0])];
check('a cyclic parent chain terminates', ids(lineageOf(cyclic, cyclic[0])).length <= 2, true);

console.log('');
if (failures) {
  console.log(`${failures} check(s) failed`);
  process.exit(1);
}
console.log('all sequence checks passed');
