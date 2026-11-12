import { test } from 'node:test';
import assert from 'node:assert/strict';
import os from 'node:os';
import fs from 'node:fs';
import path from 'node:path';
import { DEFAULT_MAC_ICON, detectMachineIcon, iconForMachine, machineIcon } from '../../src/machine.ts';
import { tmpDir } from '../helpers/fakes.ts';

test('a runner\'s icon is the kind of Mac it is on', () => {
  assert.equal(iconForMachine('MacBook Pro'), 'laptopcomputer');
  assert.equal(iconForMachine('MacBook Air'), 'laptopcomputer');
  assert.equal(iconForMachine('Mac mini'), 'macmini');
  assert.equal(iconForMachine('Mac Studio'), 'macstudio');
  assert.equal(iconForMachine('Mac Pro'), 'macpro.gen3');
  assert.equal(iconForMachine('iMac'), 'desktopcomputer');
  assert.equal(iconForMachine('Something new'), DEFAULT_MAC_ICON);
  assert.equal(iconForMachine(undefined), DEFAULT_MAC_ICON);
});

test('detecting it on this Mac gives one of the symbols', { skip: os.platform() !== 'darwin' }, async () => {
  assert.match(await detectMachineIcon(), /^(laptopcomputer|macmini|macstudio|macpro\.gen3|desktopcomputer)$/);
});

// system_profiler held each restart up to learn what never changes.
test('machineIcon remembers the icon, uses it at once and checks again in the background', async () => {
  const dir = tmpDir();
  let calls = 0;
  let answer = 'macmini';
  const detect = async () => {
    calls++;
    return answer;
  };
  assert.equal(await machineIcon(dir, detect), 'macmini');
  assert.equal(calls, 1);
  // Remembered: given straight away, with a check behind it.
  answer = 'laptopcomputer';
  assert.equal(await machineIcon(dir, detect), 'macmini');
  await new Promise((r) => setImmediate(r));
  assert.equal(calls, 2);
  // That check's answer is what the next start uses.
  assert.equal(await machineIcon(dir, detect), 'laptopcomputer');
  // Another Mac's (a data dir carried across) is worked out again first.
  fs.writeFileSync(path.join(dir, 'machine-icon.json'), JSON.stringify({ host: 'some-other-mac', icon: 'macpro.gen3' }));
  answer = 'macstudio';
  assert.equal(await machineIcon(dir, detect), 'macstudio');
});
