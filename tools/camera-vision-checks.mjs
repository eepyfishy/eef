// Private Python source/pipe only; no Rust workload/model/device integration claim.
import assert from 'node:assert/strict';
import {mkdtemp, mkdir, readFile, writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {spawn} from 'node:child_process';
import {createHash, randomUUID} from 'node:crypto';

export async function verifyCameraVision({root, python}) {
  await mkdir(join(root, '.validation'), {recursive: true});
  const scratch = await mkdtemp(join(root, '.validation/camera-vision-'));
  const sourcePaths = ['python/streams/camera_motion.py', 'python/streams/camera_vision.py',
    'python/eef_vision_stream_host.py', 'python/tests/test_camera_vision.py',
    'python/tests/camera_motion_fakes.py', 'tools/camera-vision-checks.mjs'];
  const inputs = await Promise.all(sourcePaths.map(async path => ({path,
    sha256: createHash('sha256').update(await readFile(join(root, path))).digest('hex')})));
  await writeFile(join(scratch, 'inputs.json'), JSON.stringify(inputs, null, 2), {flag: 'wx'});
  const unit = spawn(python, ['-I', '-B', '-X', 'utf8', 'python/tests/test_camera_vision.py'],
    {cwd: root, windowsHide: true, stdio: ['ignore', 'pipe', 'pipe']});
  let unitOut = '', unitErr = '';
  unit.stdout.on('data', b => unitOut += b);
  unit.stderr.on('data', b => unitErr += b);
  const unitTimer = setTimeout(() => unit.kill(), 10000);
  let unitCode;
  try {
    unitCode = await new Promise((yes, no) => { unit.once('error', no); unit.once('close', yes); });
  } finally { clearTimeout(unitTimer); }
  await writeFile(join(scratch, 'unit.json'), JSON.stringify({code: unitCode, stdout: unitOut, stderr: unitErr}, null, 2));
  assert.equal(unitCode, 0, 'Vision source units failed; retain ' + scratch);
  assert.match(unitErr, /Ran 11 tests/); assert.match(unitErr, /\nOK\s*$/);
  const motion = await readFile(join(root, sourcePaths[0]), 'utf8');
  const vision = await readFile(join(root, sourcePaths[1]), 'utf8');
  const host = await readFile(join(root, sourcePaths[2]), 'utf8');
  const fakes = await readFile(join(root, sourcePaths[4]), 'utf8');
  const cases = [];
  async function run(name, behavior, overrides = {}, replacement = '') {
    const trace = join(scratch, name + '.trace');
    const setup = `import sys as _v_sys, types as _v_types, pathlib as _v_path
_v_fake = _v_types.ModuleType('vision_device_fake')
exec(${JSON.stringify(fakes)}, _v_fake.__dict__)
def _v_log(kind):
    with _v_path.Path(${JSON.stringify(trace)}).open('a', encoding='utf8') as out: out.write(kind+'\\n')
_v_camera = _v_fake.Camera([_v_fake.Frame(n) for n in (0,20,20,255,255)])
_v_camera.on_open = lambda: _v_log('open')
_v_camera.on_read = lambda: _v_log('read')
_v_release = _v_camera.release
def _v_close():
    _v_release()
    _v_log('close')
_v_camera.release = _v_close
_v_cv = _v_fake.cv2(_v_camera)
_v_cv.IMWRITE_JPEG_QUALITY = 1
_v_bytes = bytes.fromhex('ffd8') + bytes(16) + bytes.fromhex('ffd9')
_v_cv.imencode = lambda *args: (True, _v_types.SimpleNamespace(nbytes=len(_v_bytes),tobytes=lambda: _v_bytes))
_v_sys.modules['cv2'] = _v_cv
`;
    const source = setup + '\n' + motion + '\n' + vision + '\n' + replacement;
    const stream = randomUUID();
    const request = {schema_version: 1, stream_id: stream, source_utf8: source,
      source_sha256: createHash('sha256').update(source).digest('hex'),
      params: {max_frames: 5, interval_ms: 100, max_duration_ms: 5000,
        analysis: {width: 320, height: 240, max_image_bytes: 128, jpeg_quality: 70,
          max_images: 2, min_interval_ms: 0, trigger: 'rising'}},
      bounds: {max_frame_bytes: 2048, max_stdout_bytes: 65536, max_observations: 5,
        max_source_frames: 5, max_image_bytes: 128, max_images: 2}, ...overrides};
    const child = spawn(python, ['-I', '-B', '-X', 'utf8', '-c', host, '--pinned-camera-vision-v1'],
      {cwd: root, windowsHide: true, stdio: ['pipe', 'pipe', 'pipe']});
    let output = '', stderr = '', pending = '', exited = false, wake;
    const lines = [];
    const result = new Promise((yes, no) => {
      child.once('error', no);
      child.once('close', code => { exited = true; wake?.(); yes(code); });
    });
    result.catch(() => {});
    child.stdout.on('data', bytes => {
      output += bytes; pending += bytes;
      if (Buffer.byteLength(output) > 1024 * 1024) child.kill();
      for (;;) {
        const end = pending.indexOf('\n'); if (end < 0) break;
        lines.push(pending.slice(0, end)); pending = pending.slice(end + 1);
      }
      wake?.();
    });
    child.stderr.on('data', bytes => { stderr = (stderr + bytes).slice(-16384); });
    child.stdin.on('error', () => {});
    const timer = setTimeout(() => child.kill(), 10000);
    const next = async () => {
      const end = Date.now() + 3000;
      while (!lines.length && !exited) {
        if (Date.now() >= end) throw Error('Vision fixture reply timed out: ' + name);
        await new Promise(yes => { wake = yes; setTimeout(yes, 20); });
        wake = undefined;
      }
      return lines.length ? JSON.parse(lines.shift()) : null;
    };
    const send = (type, sequence, extra = {}) => child.stdin.write(JSON.stringify({
      type, schema_version: 1, stream_id: stream, ...(sequence === undefined ? {} : {sequence}), ...extra}) + '\n');
    const traceKinds = async () => (await readFile(trace, 'utf8').catch(() => '')).trim().split(/\r?\n/).filter(Boolean);
    try {
      child.stdin.write(JSON.stringify(request) + '\n');
      const code = await behavior({next, send, child, stream, traceKinds, result});
      assert.equal(await result, code, 'Unexpected private vision exit: ' + name);
      assert(!output.includes('PRIVATE-VISION-DETAIL'));
      assert(!stderr.includes('PRIVATE-VISION-DETAIL'));
      if (code !== 0) assert.equal(stderr.trim(), 'vision_stream_failed');
      const kinds = await traceKinds();
      cases.push({name, exit_code: code, opens: kinds.filter(x => x === 'open').length,
        reads: kinds.filter(x => x === 'read').length, closes: kinds.filter(x => x === 'close').length});
    } finally {
      clearTimeout(timer); if (!exited) child.kill();
      const code = await result;
      // Retain closed observations for failed cases without serializing images.
      const records = output.trim().split(/\r?\n/).filter(Boolean).map(line => {
        try {
          const value = JSON.parse(line);
          return {kind: ['ready', 'observation', 'end'].includes(value.type) ? value.type : 'unknown',
            sequence: Number.isSafeInteger(value.sequence) ? value.sequence : null,
            image_present: typeof value.image_base64 === 'string'};
        } catch { return {kind: 'invalid_json'}; }
      });
      await writeFile(join(scratch, name + '.process.json'), JSON.stringify({code, records,
        stderr_present: stderr.length > 0, closed_error: stderr.trim() === 'vision_stream_failed'}, null, 2));
    }
  }
  const ready = async c => {
    const r = await c.next();
    assert.equal(r.type, 'ready'); assert.equal(r.protocol, 'camera_vision_v1');
    assert.equal(r.stream_id, c.stream);
    assert.deepEqual(await c.traceKinds(), []);
  };
  await run('paced', async c => {
    await ready(c); c.send('start');
    let images = 0;
    for (let sequence = 1; sequence <= 5; sequence++) {
      const record = await c.next();
      assert.equal(record.type, 'observation'); assert.equal(record.sequence, sequence);
      if (sequence === 1) {
        await new Promise(yes => setTimeout(yes, 120));
        assert.deepEqual(await c.traceKinds(), ['open', 'read']);
      }
      if (record.analysis_state === 'frame') {
        images++;
        assert.equal(record.active, true);
        assert.equal(Buffer.from(record.image_base64, 'base64').toString('hex'), 'ffd8' + '00'.repeat(16) + 'ffd9');
      } else assert.equal(record.image_base64, null);
      c.send('ack', sequence);
    }
    const end = await c.next();
    assert.deepEqual(end, {type: 'end', stream_id: c.stream, observations: 5, source_frames: 5, images: 2});
    assert.equal(images, 2); assert.equal(await c.next(), null);
    assert.deepEqual(await c.traceKinds(), ['open', 'read', 'read', 'read', 'read', 'read', 'close']);
    return 0;
  });
  await run('legacy-eof', async c => {
    await ready(c); c.child.stdin.end(); assert.equal(await c.next(), null);
    assert.deepEqual(await c.traceKinds(), []); return 1;
  });
  for (const mode of ['wrong-sequence', 'wrong-stream']) await run(mode, async c => {
    await ready(c); c.send('start'); const r = await c.next(); assert.equal(r.sequence, 1);
    c.send('ack', mode === 'wrong-sequence' ? 2 : 1, mode === 'wrong-stream' ? {stream_id: randomUUID()} : {});
    assert.equal(await c.next(), null); assert.deepEqual(await c.traceKinds(), ['open', 'read', 'close']); return 1;
  });
  await run('oversized-control', async c => {
    await ready(c); c.child.stdin.write(' '.repeat(513) + '\n'); assert.equal(await c.next(), null);
    assert.deepEqual(await c.traceKinds(), []); return 1;
  });
  await run('duplicate-control-field', async c => {
    await ready(c);
    c.child.stdin.write(`{"type":"start","type":"start","schema_version":1,"stream_id":"${c.stream}"}\n`);
    assert.equal(await c.next(), null); assert.deepEqual(await c.traceKinds(), []); return 1;
  });
  await run('bad-digest', async c => { assert.equal(await c.next(), null); return 1; }, {source_sha256: 'a'.repeat(64)});
  await run('private-source-error', async c => {
    await ready(c); c.send('start'); assert.equal(await c.next(), null); return 1;
  }, {}, `\ndef stream(action,params):\n    raise ValueError('PRIVATE-VISION-DETAIL')\n    yield\n`);
  await run('early-end', async c => {
    await ready(c); c.send('start'); const r = await c.next(); assert.equal(r.sequence, 1);
    c.send('ack', 1); assert.equal(await c.next(), null); return 1;
  }, {}, `\ndef stream(action,params):\n    yield {'source_frames':1,'active':None,'level':None,'discontinuity':False,'analysis_state':'not_requested','image_base64':None}\n`);
  await run('unsolicited-image', async c => {
    await ready(c); c.send('start'); assert.equal(await c.next(), null); return 1;
  }, {}, `\ndef stream(action,params):\n    yield {'source_frames':1,'active':None,'level':None,'discontinuity':False,'analysis_state':'not_requested','image_base64':'PRIVATE-VISION-DETAIL'}\n`);
  await writeFile(join(scratch, 'results.json'), JSON.stringify({passed: true, source_units: 11,
    protocol_cases: cases, synthetic_camera: true, real_codec: false,
    start_and_per_observation_ack_required: true, old_motion_protocol_unchanged: true,
    model_execution: false, workload_admission_integrated: false,
    physical_capture: false, stable_release_accepted: false}, null, 2), {flag: 'wx'});
  console.log('Camera vision source and private pipe fake-device validation passed: ' + scratch);
}
