// Standalone preview of the flame and strip, with a control per setting.
// Runs firmware/web's WebAssembly (served at wasm/web.wasm by `pnpm dev`).
// The values you change live in the address's #fragment, readable as
// #strip.pattern=comet&strip.secondary=ffd000, so a reload keeps them and a
// link shares them.

// [key, label, min, max, default]: the wisp tuning (WispParams).
const WISPS = [
	[ 'strands', 'Ribbons', 1, 64, 48 ],
	[ 'height', 'Height %', 10, 100, 90 ],
	[ 'sway', 'Sway', 0, 100, 50 ],
	[ 'speed', 'Speed', 0, 100, 50 ],
	[ 'glow', 'Glow', 0, 100, 50 ],
	[ 'width', 'Width', 0, 100, 50 ]
];

// The strip (strip::Params); lists are in the crate's order.
const PATTERNS = [ 'sweep', 'comet', 'converge', 'breathe', 'wave', 'heartbeat', 'twinkle' ];
const GRADIENTS = [ 'hue', 'mix' ];
const EDGES = [ 'soft', 'solid' ];
const STRIP = [
	[ 'pattern', 'Pattern', PATTERNS, 'sweep' ],
	[ 'leds', 'LEDs', 1, 300, 60 ],
	[ 'primary', 'Primary', 'color', '#0040ff' ],
	[ 'secondary', 'Secondary', 'color', '' ],
	[ 'gradient', 'Gradient', GRADIENTS, 'hue' ],
	[ 'balance', 'Balance', 0, 100, 50 ],
	[ 'sharpness', 'Sharpness', 0, 100, 0 ],
	[ 'edge', 'Edge', EDGES, 'soft' ],
	[ 'background', 'Background', 'color', '#000000' ],
	[ 'speed', 'Speed', 1, 100, 30 ],
	[ 'width', 'Width', 1, 50, 8 ],
	[ 'identify', 'Identify', 'flag', false ]
];

// The displays the flame is shown on.
const SCREENS = { tdisplay: [ 320, 170 ], lcd: [ 320, 240 ] };

const defaults = () => ({
	screen: 'tdisplay',
	wisps: Object.fromEntries(WISPS.map((f) => [ f[0], f.at(-1) ])),
	strip: Object.fromEntries(STRIP.map((f) => [ f[0], f.at(-1) ]))
});

const FIELDS = { wisps: WISPS, strip: STRIP };

// The fragment's values over the defaults; ones that do not fit a field are
// ignored, so a hand-edited link cannot break the page.
function fromHash() {
	const state = defaults();
	for (const [ name, text ] of new URLSearchParams(location.hash.slice(1))) {
		if (name == 'screen') {
			if (SCREENS[text])
				state.screen = text;
			continue;
		}
		const [ group, key ] = name.split('.');
		const field = FIELDS[group]?.find((f) => f[0] == key);
		if (!field)
			continue;
		const kind = field[2];
		if (Array.isArray(kind)) {
			if (kind.includes(text))
				state[group][key] = text;
		}
		else if (kind == 'color') {
			if (/^[0-9a-f]{6}$/i.test(text))
				state[group][key] = `#${text.toLowerCase()}`;
			else if (text == '')
				state[group][key] = '';
		}
		else if (kind == 'flag')
			state[group][key] = text == '1';
		else if (Number(text) >= kind && Number(text) <= field[3])
			state[group][key] = Number(text);
	}
	return state;
}

let state = fromHash();

// Only the values that differ from the defaults, as the settings file has them.
function changed() {
	const d = defaults();
	const pick = (group) => Object.fromEntries(Object.entries(state[group]).filter(([ k, v ]) => v !== d[group][k] && v !== ''));
	const out = {};
	for (const group of [ 'wisps', 'strip' ]) {
		const values = pick(group);
		if (Object.keys(values).length)
			out[group] = values;
	}
	return out;
}

function toHash() {
	const params = new URLSearchParams();
	if (state.screen != defaults().screen)
		params.set('screen', state.screen);
	const d = defaults();
	for (const group of [ 'wisps', 'strip' ])
		for (const [ key, v ] of Object.entries(state[group]))
			if (v !== d[group][key])
				params.set(`${group}.${key}`, typeof v == 'boolean' ? (v ? '1' : '0') : String(v).replace(/^#/, ''));
	return params.toString();
}

function save() {
	const hash = toHash();
	history.replaceState(null, '', hash ? `#${hash}` : location.pathname);
	document.getElementById('json').value = JSON.stringify(changed(), null, '\t');
	dirty = true;
}

// The controls, built from the tables above; each edits `target[key]`.
function control(target, [ key, label, kind, ...rest ]) {
	const row = document.createElement('label');
	row.className = 'row';
	const value = () => target[key];
	const set = (v) => {
		target[key] = v;
		save();
	};
	let input;
	if (Array.isArray(kind)) {
		input = document.createElement('select');
		for (const name of kind)
			input.add(new Option(name, name));
		input.value = value();
		input.oninput = () => set(input.value);
	}
	else if (kind == 'color') {
		input = document.createElement('input');
		input.type = 'color';
		// An empty secondary is "none": a switch beside the picker.
		if (key == 'secondary') {
			const on = document.createElement('input');
			on.type = 'checkbox';
			on.checked = value() != '';
			on.title = 'Use a secondary colour';
			input.value = value() || '#ffd000';
			input.disabled = !on.checked;
			on.oninput = () => {
				input.disabled = !on.checked;
				set(on.checked ? input.value : '');
			};
			input.oninput = () => set(input.value);
			row.append(label, input, on);
			return row;
		}
		input.value = value();
		input.oninput = () => set(input.value);
	}
	else if (kind == 'flag') {
		input = document.createElement('input');
		input.type = 'checkbox';
		input.checked = value();
		input.oninput = () => set(input.checked);
		input.style.justifySelf = 'start';
	}
	else {
		const [ max ] = rest;
		input = document.createElement('input');
		input.type = 'range';
		input.min = kind;
		input.max = max;
		input.value = value();
		const out = document.createElement('output');
		out.value = value();
		input.oninput = () => {
			out.value = input.value;
			set(Number(input.value));
		};
		row.append(label, input, out);
		return row;
	}
	row.append(label, input);
	return row;
}

function heading(text) {
	const h = document.createElement('h2');
	h.textContent = text;
	return h;
}

function buildControls() {
	const box = document.getElementById('controls');
	box.replaceChildren();
	box.append(heading('Flame'));
	box.append(control(state, [ 'screen', 'Display', Object.keys(SCREENS) ]));
	for (const f of WISPS)
		box.append(control(state.wisps, f));
	box.append(heading('Strip'));
	for (const f of STRIP)
		box.append(control(state.strip, f));
}

let dirty = true;

const hex = (c) => c ? parseInt(c.slice(1), 16) : -1;

function drawStrip(ctx, rgb, leds) {
	const { width, height } = ctx.canvas;
	ctx.fillStyle = '#000';
	ctx.fillRect(0, 0, width, height);
	const step = width / Math.max(leds, 1);
	const r = Math.max(1, Math.min(step * 0.38, height * 0.3));
	for (let i = 0; i < leds; i++) {
		const [ red, green, blue ] = rgb.subarray(3 * i, 3 * i + 3);
		const lit = red + green + blue > 0;
		ctx.fillStyle = lit ? `rgb(${red},${green},${blue})` : '#1c1c1c';
		ctx.shadowColor = ctx.fillStyle;
		ctx.shadowBlur = lit ? r * 2 : 0;
		ctx.beginPath();
		ctx.arc(step * (i + 0.5), height / 2, r, 0, 2 * Math.PI);
		ctx.fill();
	}
	ctx.shadowBlur = 0;
}

async function main() {
	buildControls();
	save();
	document.getElementById('reset').onclick = () => {
		state = defaults();
		buildControls();
		save();
	};
	// A link pasted into the address bar, or edited there.
	addEventListener('hashchange', () => {
		state = fromHash();
		buildControls();
		save();
	});
	document.getElementById('copy').onclick = () => navigator.clipboard?.writeText(document.getElementById('json').value);

	const bytes = await fetch('wasm/web.wasm').then((r) => r.ok ? r.arrayBuffer() : Promise.reject(new Error(`wasm/web.wasm: ${r.status}`)));
	const { instance } = await WebAssembly.instantiate(bytes, {});
	const x = instance.exports;

	const screen = document.getElementById('screen');
	const sctx = screen.getContext('2d');
	const stripCtx = document.getElementById('strip').getContext('2d');
	let image = null;
	let size = '';

	const start = performance.now();
	let last = start;
	const frame = (now) => {
		requestAnimationFrame(frame);
		const dt = Math.min(now - last, 100);
		last = now;

		if (state.screen != size) {
			size = state.screen;
			const [ w, h ] = SCREENS[size];
			screen.width = w;
			screen.height = h;
			x.wisps_new(w, h);
			image = sctx.createImageData(w, h);
			dirty = true;
		}
		if (dirty) {
			const w = state.wisps, s = state.strip;
			x.wisps_params(w.strands, w.height, w.sway, w.speed, w.glow, w.width);
			x.strip_params(PATTERNS.indexOf(s.pattern), s.speed, s.width, hex(s.primary), hex(s.secondary),
				GRADIENTS.indexOf(s.gradient), s.balance, s.sharpness, EDGES.indexOf(s.edge), hex(s.background));
			dirty = false;
		}

		image.data.set(new Uint8Array(x.memory.buffer, x.wisps_frame(Math.round(dt)), image.data.length));
		sctx.putImageData(image, 0, 0);

		const leds = state.strip.leds;
		const rgb = new Uint8Array(x.memory.buffer, x.strip_frame(leds, now - start), leds * 3).slice();
		// The counting pattern (the Zero's identify()): first LED green, every
		// 10th red, the rest dim blue.
		if (state.strip.identify)
			for (let i = 0; i < leds; i++)
				rgb.set(i == 0 ? [ 0, 255, 0 ] : (i + 1) % 10 == 0 ? [ 255, 0, 0 ] : [ 0, 0, 64 ], 3 * i);
		drawStrip(stripCtx, rgb, leds);
	};
	requestAnimationFrame(frame);
}

main().catch((e) => {
	document.getElementById('error').textContent = `Preview unavailable: ${e.message}`;
});
