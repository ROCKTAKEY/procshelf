import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdtemp, writeFile, rename, rm, access } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { createServer } from 'node:net';
import { setTimeout as delay } from 'node:timers/promises';

async function eventually(fn, ms = 5000) {
  const deadline = Date.now() + ms;
  while (Date.now() < deadline) { if (await fn()) return; await delay(30); }
  throw new Error('condition did not become true');
}
async function unusedPort() {
  const server = createServer();
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const port = server.address().port;
  await new Promise(resolve => server.close(resolve));
  return port;
}
async function fixture(t, body) {
  const dir = await mkdtemp(join(tmpdir(), 'procshelf-test-'));
  const port = await unusedPort();
  const config = join(dir, 'config.toml');
  const text = `[web]\nlisten="127.0.0.1:${port}"\n${body}`;
  await writeFile(config, text);
  const children = [];
  const launch = () => {
    const child = spawn(resolve('target/debug/procshelf'), ['--config', config], { stdio: ['ignore', 'ignore', 'pipe'] });
    child.diagnostics = '';
    child.stderr.on('data', data => { child.diagnostics += data.toString(); });
    child.done = new Promise(resolve => child.once('exit', (code, signal) => resolve({ code, signal })));
    children.push(child); return child;
  };
  t.after(async () => {
    for (const child of children) {
      if (child.exitCode === null && child.signalCode === null) child.kill('SIGTERM');
      await Promise.race([child.done, delay(5000).then(() => child.kill('SIGKILL'))]);
    }
    await rm(dir, {recursive: true, force: true});
  });
  const child = launch();
  const base = `http://127.0.0.1:${port}`;
  await eventually(async () => {try {return (await fetch(`${base}/api/state`)).ok;} catch {return false;}});
  const state = () => fetch(`${base}/api/state`).then(r => r.json());
  const {token} = await fetch(`${base}/api/session`).then(r => r.json());
  async function act(action, snapshot) {
    const s = snapshot ?? await state();
    return fetch(`${base}/api/processes/api/actions`, {
      method: 'POST', headers: {'Content-Type':'application/json','X-Procshelf-Token':token},
      body: JSON.stringify({action, expected_run_id:s.processes[0].run_id, config_generation:s.config_generation}),
    });
  }
  return {dir,config,text,child,launch,base,state,act};
}
const command = `[processes.api]\nprogram="/bin/sh"\nargs=["-c", "echo ready; sleep 60"]\nstop_timeout_seconds=1\n`;

test('HTTP controls, live configuration, and single-owner lock work together', {timeout:15000}, async t => {
  const f = await fixture(t, command);
  assert.equal((await f.state()).processes[0].state, 'stopped');
  const old = await f.state();
  const responses = await Promise.all([f.act('start', old), f.act('start', old)]);
  assert.deepEqual(responses.map(r => r.status).sort(), [202,409]);
  await eventually(async () => (await f.state()).processes[0].state === 'running');
  const pid = (await f.state()).processes[0].pid;
  const duplicate = f.launch();
  assert.equal((await duplicate.done).code, 1);
  assert.match(duplicate.diagnostics, /already managed/);
  const replacement = join(f.dir, 'replacement');
  await writeFile(replacement, f.text + 'name="Changed live"\n');
  const saved = Date.now();
  await rename(replacement, f.config);
  await eventually(async () => (await f.state()).processes[0].name === 'Changed live');
  assert.ok(Date.now() - saved < 1000);
  assert.equal((await f.state()).processes[0].pid, pid);
  await writeFile(f.config, '[broken');
  await eventually(async () => (await f.state()).config_error !== null);
  assert.equal((await f.state()).processes[0].pid, pid);
  await writeFile(f.config, f.text);
  await eventually(async () => (await f.state()).config_error === null);
  assert.equal((await f.act('stop')).status, 202);
  await eventually(async () => (await f.state()).processes[0].state === 'exited');
});

test('a crashed manager retains its configuration lease until supervisors finish cleanup', {timeout:15000}, async t => {
  const f = await fixture(t, `[processes.api]\nprogram="/bin/sh"\nargs=["-c", "trap '' TERM; echo ready; sleep 60"]\nautostart=true\nstop_timeout_seconds=1\n`);
  await eventually(async () => (await f.state()).processes[0].state === 'running');
  await eventually(async () => (await fetch(`${f.base}/api/processes/api/logs`).then(r=>r.json())).entries.some(e=>e.text.includes('ready')));
  const pid = (await f.state()).processes[0].pid;
  f.child.kill('SIGKILL'); await f.child.done;
  const duplicate = f.launch();
  await Promise.race([duplicate.done, delay(300)]);
  assert.equal(duplicate.exitCode, 1, 'must not start another copy while previous run is stopping');
  assert.match(duplicate.diagnostics, /already managed/);
  await eventually(async () => {try {await access(`/proc/${pid}`);return false;}catch{return true;}});
});
