import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import {
  chmodSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { test } from 'node:test'

const source = readFileSync(new URL('./deploy-server-release.sh', import.meta.url), 'utf8')

function fixture() {
  const root = mkdtempSync(path.join(os.tmpdir(), 'mediagen deploy '))
  for (const name of [
    'scripts',
    'apps/console/node_modules',
    'apps/console/dist',
    'crates',
    'plugins',
    'fake-bin',
  ]) {
    mkdirSync(path.join(root, name), { recursive: true })
  }
  for (const name of [
    'Cargo.toml',
    'Cargo.lock',
    'rust-toolchain.toml',
    'package.json',
    'package-lock.json',
  ]) {
    writeFileSync(path.join(root, name), '{}')
  }
  writeFileSync(path.join(root, 'scripts/deploy-server-release.sh'), source)
  for (const name of ['apps/changed.rs', 'apps/new-untracked.rs', 'plugins/manifest.json']) {
    writeFileSync(path.join(root, name), 'source')
  }
  for (const name of [
    'apps/.env.secret',
    'apps/private.key',
    'apps/cert.pem',
    'apps/console/node_modules/private.js',
    'apps/console/dist/stale.js',
  ]) {
    writeFileSync(path.join(root, name), 'must not upload')
  }
  writeFileSync(
    path.join(root, '.env.deploy.local'),
    'touch "' + path.join(root, 'env-executed') + '"\n'
  )
  const fake = path.join(root, 'fake-bin/ssh')
  writeFileSync(
    fake,
    '#!' +
      process.execPath +
      '\n' +
      [
        "const fs = require('node:fs'), path = require('node:path')",
        'const args = process.argv.slice(2), root = process.env.MEDIAGEN_DEPLOY_TEST_ROOT',
        "fs.appendFileSync(path.join(root, 'ssh.jsonl'), JSON.stringify(args) + '\\n')",
        "if (args.some(arg => arg.includes('cat > /var/cache/mediagen/incoming/'))) {",
        "  fs.writeFileSync(path.join(root, 'uploaded.tar.gz'), fs.readFileSync(0))",
        "} else if (args.includes('bash')) {",
        "  fs.writeFileSync(path.join(root, 'remote.sh'), fs.readFileSync(0))",
        '  process.exit(Number(process.env.MEDIAGEN_DEPLOY_TEST_REMOTE_EXIT || 0))',
        '}',
      ].join('\n')
  )
  chmodSync(fake, 0o755)
  return {
    root,
    run(args = [], extra = {}) {
      return spawnSync('bash', [path.join(root, 'scripts/deploy-server-release.sh'), ...args], {
        encoding: 'utf8',
        env: {
          ...process.env,
          PATH: path.join(root, 'fake-bin') + ':' + process.env.PATH,
          MEDIAGEN_DEPLOY_TEST_ROOT: root,
          ...extra,
        },
      })
    },
  }
}

test('默认密钥部署只上传源码，包含未提交文件，忽略旧环境选项和敏感文件', () => {
  const f = fixture()
  try {
    const result = f.run([], {
      MEDIAGEN_DEPLOY_HOST: 'retired.example',
      MEDIAGEN_DEPLOY_PORT: '22',
    })
    assert.equal(result.status, 0, result.stderr)
    const calls = readFileSync(path.join(f.root, 'ssh.jsonl'), 'utf8')
      .trim()
      .split('\n')
      .map(JSON.parse)
    assert.equal(calls.length, 3)
    for (const call of calls) {
      assert.ok(call.includes('root@8.148.255.185'))
      assert.equal(call[call.indexOf('-p') + 1], '26822')
      assert.ok(call.includes('PasswordAuthentication=no'))
    }
    assert.equal(existsSync(path.join(f.root, 'env-executed')), false)
    const archive = spawnSync('tar', ['-tzf', path.join(f.root, 'uploaded.tar.gz')], {
      encoding: 'utf8',
    })
    assert.equal(archive.status, 0, archive.stderr)
    assert.match(archive.stdout, /apps\/changed\.rs/)
    assert.match(archive.stdout, /apps\/new-untracked\.rs/)
    assert.doesNotMatch(archive.stdout, /node_modules|\/dist\/|\.env|\.key|\.pem/)
    assert.equal(spawnSync('bash', ['-n', path.join(f.root, 'remote.sh')]).status, 0)
  } finally {
    rmSync(f.root, { recursive: true, force: true })
  }
})

test('非法参数在 SSH 前失败，覆盖主机端口有效，远程失败不会报成功', () => {
  const f = fixture()
  try {
    for (const args of [
      ['-bad'],
      ['host;id'],
      ['server', '0'],
      ['server', '65536'],
      ['server', '22x'],
      ['server', '22', 'extra'],
    ]) {
      assert.notEqual(f.run(args).status, 0)
      assert.equal(existsSync(path.join(f.root, 'ssh.jsonl')), false)
    }
    const result = f.run(['example.test', '2222'], { MEDIAGEN_DEPLOY_TEST_REMOTE_EXIT: '42' })
    assert.equal(result.status, 42)
    assert.doesNotMatch(result.stdout, /部署完成：http/)
    const call = JSON.parse(readFileSync(path.join(f.root, 'ssh.jsonl'), 'utf8').split('\n')[0])
    assert.ok(call.includes('root@example.test'))
    assert.equal(call[call.indexOf('-p') + 1], '2222')
  } finally {
    rmSync(f.root, { recursive: true, force: true })
  }
})

// 隔离执行实际切换代码，故意让健康检查失败，验证旧程序和数据库保护。
test('健康检查失败自动恢复旧程序，数据库和密钥完整保留', () => {
  const root = mkdtempSync(path.join(os.tmpdir(), 'mediagen rollback '))
  try {
    const installer = source
      .split("<<'INSTALL'\n")[1]
      .split('\nINSTALL\n')[0]
      .replaceAll("'/opt/mediagen'", "str(root / 'app')")
      .replaceAll("'/opt/mediagen-src'", "str(root / 'src')")
      .replaceAll("'/var/lib/mediagen'", "str(root / 'data')")
      .replaceAll(
        "'/var/cache/mediagen/cargo-target/release/mediagen-server'",
        "str(root / 'cache/server')"
      )
    const harness = [
      'import pathlib, sqlite3, sys, types, pwd, os, subprocess, urllib.request, time',
      'root = pathlib.Path(' + JSON.stringify(root) + ')',
      "app, source, data, cache = [root / name for name in ['app', 'src', 'data', 'cache']]",
      "for part in ['bin', 'console', 'plugins']:",
      '    (app / part).mkdir(parents=True)',
      "    (app / part / 'old').write_text(part)",
      "for part in ['apps/console/dist', 'plugins']:",
      '    (source / part).mkdir(parents=True)',
      "(source / 'apps/console/dist/index.html').write_text('new frontend')",
      "(source / 'plugins/manifest.json').write_text('{}')",
      'cache.mkdir()',
      "(cache / 'server').write_text('new binary')",
      'data.mkdir()',
      "(data / 'generation-queue.key').write_text('test-key')",
      "for name in ['mediagen.sqlite', 'admin.sqlite']:",
      '    with sqlite3.connect(data / name) as c:',
      "        c.execute('create table evidence (value text)')",
      "        c.execute('insert into evidence values (?)', ('keep accounts',))",
      'calls = []',
      'def command(args, **kwargs):',
      '    calls.append(args)',
      '    return types.SimpleNamespace(returncode=0)',
      'subprocess.run = command',
      'pwd.getpwnam = lambda name: types.SimpleNamespace(pw_uid=os.getuid(), pw_gid=os.getgid())',
      'time.sleep = lambda seconds: None',
      'def unavailable(*args, **kwargs):',
      "    raise OSError('injected health failure')",
      'urllib.request.build_opener = lambda *args: types.SimpleNamespace(open=unavailable)',
      "sys.argv = ['install', 'test-release']",
      'try:',
      '    exec(' + JSON.stringify(installer) + ')',
      'except RuntimeError as error:',
      "    assert '健康检查' in str(error)",
      'else:',
      "    raise AssertionError('deployment must fail')",
      "for part in ['bin', 'console', 'plugins']:",
      "    assert (app / part / 'old').read_text() == part",
      "for name in ['mediagen.sqlite', 'admin.sqlite']:",
      "    for db in [data / name, app / 'releases/test-release' / name]:",
      '        with sqlite3.connect(db) as c:',
      "            assert c.execute('select value from evidence').fetchone()[0] == 'keep accounts'",
      "assert (data / 'generation-queue.key').read_text() == 'test-key'",
      "assert calls[-1] == ['systemctl', 'start', 'mediagen-server']",
    ].join('\n')
    const result = spawnSync('python3', ['-c', harness], { encoding: 'utf8' })
    assert.equal(result.status, 0, result.stderr)
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})
