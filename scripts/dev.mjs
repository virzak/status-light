// Development loop: rebuild on every change and reload the browser.
//
//   pnpm dev
//
// serves the standalone preview (firmware/web/www) at http://localhost:3000:
// the flame and the strip with a control per setting, running the WebAssembly
// build of the shared crates. Changes to those crates rebuild it; changes to
// the page reload it. No router needed.
//
//   ROUTER=user@router-address LUCI_URL=http://router-address:8080 pnpm dev
//
// also builds the LuCI settings page, copies it to the router on every change,
// and relays the router's LuCI at the same address (/cgi-bin/luci, after
// logging in there), with caching off, since LuCI loads the page's JavaScript
// under a version tag that does not change between builds.
//
// --deploy insists on a router, so a missing ROUTER is an error rather than a
// quiet preview-only run (for tasks that pass the environment through).
//
// ROUTER is the ssh target (with sudo on the router, as in the README).
// LUCI_URL defaults to http:// plus ROUTER's host; GL.iNet firmware moves
// LuCI to port 8080. SSH_OPTS adds ssh options, split on spaces. LISTEN adds
// addresses of this computer to serve on as well as localhost, comma separated,
// e.g. its LAN address to try the pages from a phone.

import { spawn } from 'node:child_process';
import { createServer, connect } from 'node:net';
import { readdirSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
import browserSync from 'browser-sync';

const router = process.env.ROUTER;
if (process.argv.includes('--deploy') && !router) {
  console.error('[dev] --deploy needs ROUTER, the router\'s ssh target, e.g. ROUTER=user@router-address');
  process.exit(1);
}
const luciUrl = router && (process.env.LUCI_URL || `http://${router.replace(/^.*@/, '')}`);
const sshOpts = (process.env.SSH_OPTS || '').split(' ').filter(Boolean);
const extraHosts = (process.env.LISTEN || '').split(',').map((h) => h.trim()).filter(Boolean);

const PREVIEW = 'firmware/web/www';
const WASM = 'firmware/web/target/wasm32-unknown-unknown/release';

// Sources whose change means a new build. luci/www is left out: it is the
// build's output.
const BUILD_WATCH = [
  'firmware/web/src/**',
  'firmware/web/Cargo.toml',
  'firmware/flame/src/**',
  'firmware/strip/src/**',
  ...(router ? [ 'luci-src/**/*.ts', 'luci/usr/**' ] : [])
];

// A shell command (pnpm is a script on Windows, so it needs one).
function run(command) {
  return new Promise((resolve, reject) => {
    const p = spawn(command, { stdio: 'inherit', shell: true });
    p.on('error', reject);
    p.on('exit', (code) => code == 0 ? resolve() : reject(new Error(`${command} exited with ${code}`)));
  });
}

// Every file under luci/, so existing directories on the router keep their
// owner and permissions (as the README's tar command does).
function luciFiles(dir = 'luci') {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    return statSync(path).isDirectory() ? luciFiles(path) : [ relative('luci', path).replaceAll('\\', '/') ];
  });
}

function deploy() {
  return new Promise((resolve, reject) => {
    const tar = spawn('tar', [ 'cf', '-', '-C', 'luci', ...luciFiles() ], { stdio: [ 'ignore', 'pipe', 'inherit' ] });
    const ssh = spawn('ssh', [ ...sshOpts, router,
      'sudo sh -c "tar xof - -C / && rm -f /tmp/luci-indexcache* && rm -rf /tmp/luci-modulecache && /etc/init.d/rpcd reload"' ],
      { stdio: [ 'pipe', 'inherit', 'inherit' ] });
    tar.stdout.pipe(ssh.stdin);
    tar.on('error', reject);
    ssh.on('error', reject);
    ssh.on('exit', (code) => code == 0 ? resolve() : reject(new Error(`ssh exited with ${code}`)));
  });
}

const bs = browserSync.create();

// One build at a time; changes made during a build get one more afterwards.
let building = false;
let again = false;
async function rebuild() {
  if (building) {
    again = true;
    return;
  }
  building = true;
  do {
    again = false;
    const start = Date.now();
    try {
      if (router) {
        await run('pnpm run --silent luci');
        await deploy();
      }
      else
        await run('pnpm run --silent wasm');
      console.log(`[dev] ${router ? 'built and deployed' : 'built'} in ${((Date.now() - start) / 1000).toFixed(1)} s, reloading`);
      bs.reload();
    }
    catch (e) {
      console.error(`[dev] ${e.message}; waiting for the next change`);
      bs.notify('Build failed, see the terminal', 5000);
    }
  } while (again);
  building = false;
}

// Always the latest build, never a cached one.
const noStore = (_req, res, next) => {
  res.setHeader('Cache-Control', 'no-store');
  next();
};

bs.init({
  // The preview at /, its WebAssembly at /wasm; with a router, everything
  // else (LuCI) is relayed to it.
  ...(router
    ? {
      proxy: {
        target: luciUrl,
        proxyRes: [ (res) => {
          res.headers['cache-control'] = 'no-store';
          delete res.headers['etag'];
          delete res.headers['last-modified'];
        } ]
      },
      serveStatic: [ PREVIEW, { route: '/wasm', dir: WASM } ]
    }
    : { server: { baseDir: PREVIEW, routes: { '/wasm': WASM } } }),
  middleware: [ noStore ],
  // Only this computer: with a router, this is a way into its admin pages.
  listen: 'localhost',
  open: false,
  notify: true,
  ui: false,
  ghostMode: false,
  logPrefix: 'dev'
}, () => {
  // browser-sync serves one address; relay the others to it, connection by
  // connection, so the reload channel works through them too.
  const port = bs.getOption('port');
  for (const host of extraHosts)
    createServer((client) => {
      const upstream = connect(port, 'localhost');
      client.pipe(upstream).pipe(client);
      client.on('error', () => upstream.destroy());
      upstream.on('error', () => client.destroy());
    }).on('error', (e) => console.error(`[dev] cannot serve on ${host}: ${e.message}`))
      .listen(port, host, () => console.log(`[dev] also serving on http://${host}:${port}`));
  if (router)
    console.log(`[dev] LuCI (log in there): http://localhost:${port}/cgi-bin/luci`);
  rebuild();
});

let timer;
bs.watch(BUILD_WATCH, { ignoreInitial: true }, () => {
  clearTimeout(timer);
  timer = setTimeout(rebuild, 200);
});
bs.watch(`${PREVIEW}/**`, { ignoreInitial: true }, () => bs.reload());
