import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { loadConfig } from '../../src/config.ts';
import { discoverAccounts } from '../../src/accounts.ts';
import { tmpDir } from '../helpers/fakes.ts';

test('account discovery keeps the HOME and PATH passed to loadConfig', () => {
  const home = tmpDir();
  const bin = path.join(home, '.opencode', 'bin', 'opencode');
  fs.mkdirSync(path.dirname(bin), { recursive: true });
  fs.writeFileSync(bin, '#!/bin/sh\nexit 0\n', { mode: 0o755 });
  fs.mkdirSync(path.join(home, '.claude'));
  fs.writeFileSync(path.join(home, '.claude.json'), JSON.stringify({ oauthAccount: { emailAddress: 'fixture@example.com' } }));
  fs.mkdirSync(path.join(home, '.codex'));
  // The marker is checked for existence only, never opened.
  fs.writeFileSync(path.join(home, '.codex', 'auth.json'), '');
  const cfg = loadConfig({ HOME: home, PATH: '', THREADBEAR_HOME: path.join(home, 'data') });
  assert.equal(cfg.claudeMainDir, path.join(home, '.claude'));
  assert.equal(cfg.codexMainDir, path.join(home, '.codex'));
  assert.equal(cfg.defaults.cwd, home);
  assert.deepEqual(discoverAccounts(cfg).map((a) => [a.id, a.configDir]), [
    ['claude-main', path.join(home, '.claude')],
    ['codex-main', path.join(home, '.codex')],
    ['opencode-main', path.join(home, '.config', 'opencode')],
  ]);
  fs.unlinkSync(bin);
  assert.deepEqual(discoverAccounts(cfg).map((a) => a.id), ['claude-main', 'codex-main'], 'no fallback to the process PATH');
});

const preload = path.join(import.meta.dirname, '../helpers/clean-env.ts');
function child(source: string, env: NodeJS.ProcessEnv = {}) {
  return spawnSync(process.execPath, ['--import', preload, '--input-type=module', '-e', source], { encoding: 'utf8', env: { ...process.env, ...env } });
}

test('unit-test preload replaces inherited home and account overrides', () => {
  const result = child(`
    import assert from 'node:assert/strict';
    import os from 'node:os';
    const home = process.env.HOME;
    assert.notEqual(home, '/outside/home');
    assert.equal(os.homedir(), home);
    assert.ok(os.tmpdir().startsWith(home + '/'));
    for (const key of ['XDG_CONFIG_HOME', 'XDG_DATA_HOME', 'XDG_CACHE_HOME', 'XDG_STATE_HOME', 'CODEX_HOME']) assert.ok(process.env[key].startsWith(home + '/'), key);
    for (const key of ['THREADBEAR_HOME', 'THREADBARE_HOME', 'THREADBEAR_CLAUDE_MAIN_JSON', 'THREADBEAR_CODEX_MAIN_DIR', 'CLAUDE_CONFIG_DIR', 'OPENCODE_CONFIG']) assert.equal(process.env[key], undefined, key);
  `, { HOME: '/outside/home', XDG_CONFIG_HOME: '/outside/config', CODEX_HOME: '/outside/codex', CLAUDE_CONFIG_DIR: '/outside/claude', THREADBEAR_HOME: '/outside/data', THREADBARE_HOME: '/outside/old', THREADBEAR_CLAUDE_MAIN_JSON: '/outside/claude.json', THREADBEAR_CODEX_MAIN_DIR: '/outside/codex', OPENCODE_CONFIG: '/outside/opencode.json' });
  assert.equal(result.status, 0, result.stderr);
});

test('unit-test guard fails even when a real-account read error is caught', () => {
  for (const operation of [
    `fs.existsSync(file)`,
    `fs.readFileSync(new URL('file://' + file))`,
    `await fs.promises.readFile(file)`,
    `await fs.promises.readFile(file).catch(() => {})`,
    `statSync(file)`,
    `fs.realpathSync.native(file)`,
    `fs.copyFileSync(file, process.env.HOME + '/copy')`,
    `fs.copyFileSync(process.env.HOME + '/missing', file)`,
    `fs.writeFileSync(file, '')`,
  ]) {
    const result = child(`
      import fs, { statSync } from 'node:fs';
      import os from 'node:os';
      const file = os.userInfo().homedir + '/.codex/threadbear-unit-guard-probe';
      try { ${operation}; } catch {}
    `);
    assert.equal(result.status, 1, operation + ': ' + result.stderr);
    assert.match(result.stderr, /Unit tests accessed real account folders/);
  }
});

test('unit-test guard permits its update clone, filters release PATHs, and keeps sibling data protected', () => {
  // Simulate an installed update clone entirely under a fixture home.
  const home = tmpDir();
  const data = path.join(home, 'Library/Application Support/Threadbear');
  const clone = path.join(data, 'runner/update/src');
  const helpers = path.join(clone, 'runner/test/helpers');
  fs.mkdirSync(helpers, { recursive: true });
  fs.mkdirSync(path.join(home, '.codex'));
  for (const file of ['private-home.ts', 'clean-env.ts']) fs.copyFileSync(path.join(import.meta.dirname, '../helpers', file), path.join(helpers, file));
  fs.writeFileSync(path.join(clone, 'fixture.txt'), 'source');
  fs.symlinkSync(path.join(home, '.codex'), path.join(clone, 'login-link'));
  const result = spawnSync(process.execPath, ['--input-type=module', '-e', `
    import assert from 'node:assert/strict';
    import fs from 'node:fs';
    import os from 'node:os';
    const user = os.userInfo();
    os.userInfo = () => ({ ...user, homedir: ${JSON.stringify(home)} });
    await import(${JSON.stringify(path.join(helpers, 'clean-env.ts'))});
    process.chdir(${JSON.stringify(path.join(clone, 'runner'))});
    assert.equal(fs.readFileSync(${JSON.stringify(path.join(clone, 'fixture.txt'))}, 'utf8'), 'source');
    assert.equal(process.env.PATH, '/usr/bin');
    assert.doesNotThrow(() => fs.symlinkSync('../../../../app/Fixtures', process.env.HOME + '/relative-link'));
    assert.throws(() => fs.existsSync(${JSON.stringify(path.join(data, 'secrets/probe'))}), /real account folder/);
    assert.throws(() => fs.existsSync(${JSON.stringify(path.join(clone, 'login-link/probe'))}), /real account folder/);
  `], { encoding: 'utf8', env: { ...process.env, PATH: path.join(data, 'runner/releases/fixture/bin') + ':/usr/bin' } });
  assert.equal(result.status, 1, result.stderr);
  assert.match(result.stderr, /Unit tests accessed real account folders/);
  assert.doesNotMatch(result.stderr, /AssertionError/);
});

test('unit-test preload works without a passwd entry or an existing HOME', () => {
  const incoming = path.join(tmpDir(), 'nonexistent');
  const result = spawnSync(process.execPath, ['--input-type=module', '-e', `
    import assert from 'node:assert/strict';
    import os from 'node:os';
    os.userInfo = () => { throw new Error('uid has no passwd entry'); };
    await import(${JSON.stringify(preload)});
    assert.notEqual(os.homedir(), ${JSON.stringify(incoming)});
    assert.equal(os.homedir(), process.env.HOME);
  `], { encoding: 'utf8', env: { ...process.env, HOME: incoming } });
  assert.equal(result.status, 0, result.stderr);
});

test('unit-test workers preserve guarded caller homes across preloads', () => {
  for (const noPasswd of [false, true]) {
    const incoming = tmpDir();
    const worker = path.join(incoming, 'worker.test.mjs');
    const probe = path.join(incoming, '.claude', 'guard-probe');
    fs.mkdirSync(path.dirname(probe));
    fs.writeFileSync(probe, 'fixture');
    const read = `try { fs.existsSync(${JSON.stringify(probe)}); } catch {}`;
    fs.writeFileSync(worker, `import { test } from 'node:test'; import fs from 'node:fs'; test('a caught account probe still fails the worker', () => { ${read} });`);
    const imports: string[] = [];
    if (noPasswd) {
      const bootstrap = path.join(incoming, 'no-passwd.mjs');
      fs.writeFileSync(bootstrap, `import os from 'node:os'; os.userInfo = () => { throw new Error('uid has no passwd entry'); };`);
      imports.push('--import', bootstrap);
    }
    imports.push('--import', preload);
    for (const asTest of [false, true]) {
      const inner = [...imports, ...(asTest ? ['--test', worker] : ['--input-type=module', '-e', `import fs from 'node:fs'; ${read}`])];
      // Launch with the caller's HOME, then again after the first preload
      // replaces it. --test adds its own worker preload as well.
      const result = spawnSync(process.execPath, [...imports, '--input-type=module', '-e', `
        import { spawnSync } from 'node:child_process';
        const result = spawnSync(process.execPath, ${JSON.stringify(inner)}, { encoding: 'utf8', env: process.env });
        process.stdout.write(result.stdout ?? '');
        process.stderr.write(result.stderr ?? '');
        process.exitCode = result.status ?? 1;
      `], {
        encoding: 'utf8',
        env: { ...process.env, HOME: incoming, USERPROFILE: incoming, NODE_TEST_CONTEXT: undefined, THREADBEAR_TEST_GUARDED_HOMES: undefined },
      });
      const output = result.stdout + result.stderr;
      assert.equal(result.status, 1, `noPasswd=${noPasswd}, asTest=${asTest}: ${output}`);
      assert.match(output, /Unit tests accessed real account folders/);
      assert.ok(output.includes(probe), output);
    }
  }
});

test('unit-test guard follows a fixture symlink back to a real account', () => {
  for (const replace of [
    `fs.unlinkSync(alias); fs.symlinkSync(os.userInfo().homedir, alias);`,
    `execFileSync('/bin/ln', ['-sfn', os.userInfo().homedir, alias]);`,
  ]) {
    const result = child(`
      import fs from 'node:fs';
      import os from 'node:os';
      import { execFileSync } from 'node:child_process';
      const alias = process.env.HOME + '/alias';
      fs.symlinkSync(process.env.HOME, alias);
      fs.existsSync(alias + '/.claude/threadbear-unit-guard-probe');
      ${replace}
      try { fs.existsSync(alias + '/.claude/threadbear-unit-guard-probe'); } catch {}
    `);
    assert.equal(result.status, 1, result.stderr);
    assert.match(result.stderr, /Unit tests accessed real account folders/);
  }
});
