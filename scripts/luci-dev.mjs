// Settings page development loop: rebuild on every change, copy the result to
// the router, and reload the page in the browser.
//
//   ROUTER=user@router-address LUCI_URL=http://router-address:8080 pnpm dev
//
// Then open the address it prints (http://localhost:3000) and log in to LuCI
// there. That address relays to the router's LuCI, adds browser-sync's reload
// script to each page and turns off caching, since LuCI loads the page's
// JavaScript under a version tag that does not change between builds.
//
// ROUTER is the ssh target (with sudo on the router, as in the README).
// LUCI_URL defaults to http:// plus ROUTER's host; GL.iNet firmware moves
// LuCI to port 8080. SSH_OPTS adds ssh options, split on spaces.

import { spawn } from 'node:child_process';
import { readdirSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
import browserSync from 'browser-sync';

const router = process.env.ROUTER;
if (!router) {
	console.error('Set ROUTER to the router\'s ssh target, e.g. ROUTER=user@router-address');
	process.exit(1);
}
const luciUrl = process.env.LUCI_URL || `http://${router.replace(/^.*@/, '')}`;
const sshOpts = (process.env.SSH_OPTS || '').split(' ').filter(Boolean);

// Sources whose change means a new build. luci/www is left out: it is the
// build's output.
const WATCH = [
	'luci-src/**/*.ts',
	'luci/usr/**',
	'firmware/web/src/**',
	'firmware/web/Cargo.toml',
	'firmware/flame/src/**',
	'firmware/strip/src/**'
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
			await run('pnpm run --silent luci');
			await deploy();
			console.log(`[luci-dev] deployed in ${((Date.now() - start) / 1000).toFixed(1)} s, reloading`);
			bs.reload();
		}
		catch (e) {
			console.error(`[luci-dev] ${e.message}; waiting for the next change`);
			bs.notify('Build failed, see the terminal', 5000);
		}
	} while (again);
	building = false;
}

bs.init({
	proxy: {
		target: luciUrl,
		proxyRes: [ (res) => {
			res.headers['cache-control'] = 'no-store';
			delete res.headers['etag'];
			delete res.headers['last-modified'];
		} ]
	},
	// Only this computer: the proxy is a way into the router's admin pages.
	listen: 'localhost',
	open: false,
	notify: true,
	ui: false,
	ghostMode: false,
	logPrefix: 'luci-dev'
}, () => rebuild());

let timer;
bs.watch(WATCH, { ignoreInitial: true }, () => {
	clearTimeout(timer);
	timer = setTimeout(rebuild, 200);
});
