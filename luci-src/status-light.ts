import view from 'luci/view';
import form from 'luci/form';
import fs from 'luci/fs';
import ui from 'luci/ui';

// Edits /etc/status-light.json (settings.schema.json in the status-light repo)
// with a JSONMap. JSONMap keeps its data in memory and its save() does nothing,
// so handleSave() collects the edited sections, restores numbers (the form
// stores every value as a string) and writes the file itself. netled and
// flame-screen notice the new file within a few seconds.
//
// Compiled into luci/www/luci-static/resources/view/status-light.js by
// `pnpm luci` (see the README); edit this file, not the .js.

const FILE = '/etc/status-light.json';
const SCHEMA = 'https://raw.githubusercontent.com/virzak/status-light/master/settings.schema.json';
const USB = '/sys/bus/usb/devices';
// Installed with flame-screen, which draws on a router's own LCD (GL.iNet Flint
// 4, under devices/); the page shows the LCD settings only when it is there.
const FLAME_SCREEN = '/etc/init.d/flame-screen';

/** A board or LCD entry in the settings file. */
interface Entry {
	name?: string;
	brightness?: number;
	strip?: Record<string, number | boolean | string>;
	wisps?: Record<string, number>;
}

/** /etc/status-light.json, as settings.schema.json describes it. */
interface Settings {
	$schema?: string;
	lcd?: Entry;
	boards?: Record<string, Entry>;
}

/** An entry flattened for the form ({ wisps: { sway } } -> { wisps_sway }). */
type FormSection = Record<string, unknown>;

/** [key, label, validator, description]; a validator of 'flag' is an on/off
 * switch, stored as a JSON boolean, 'choice' one of CHOICES[key], stored as its
 * name, and 'color' a #rrggbb colour, stored as that string. */
type Field = readonly [key: string, label: string, datatype: string, description?: string];

/** A USB status board currently plugged in. */
interface Board {
	serial: string;
	product: string;
}

// The wisp parameters (WispParams), the flame the LCDs show.
const WISPS: readonly Field[] = [
	[ 'strands', _('Ribbons'), 'range(1,64)' ],
	[ 'height', _('Height (% of the screen)'), 'range(10,100)' ],
	[ 'sway', _('Sway'), 'range(0,100)' ],
	[ 'speed', _('Speed'), 'range(0,100)' ],
	[ 'glow', _('Glow'), 'range(0,100)' ],
	[ 'width', _('Ribbon width'), 'range(0,100)' ]
];

// What the strip can show while online (the strip crate's Pattern), and which
// of them use the width setting.
const PATTERNS: readonly [ name: string, label: string ][] = [
	[ 'sweep', _('Sweep: a glow sliding end to end') ],
	[ 'comet', _('Comet: a head with a fading tail') ],
	[ 'converge', _('Converge: glows meeting in the middle') ],
	[ 'breathe', _('Breathe: the whole strip fading in and out') ],
	[ 'wave', _('Wave: a wave travelling along') ],
	[ 'heartbeat', _('Heartbeat: a double pulse') ],
	[ 'twinkle', _('Twinkle: LEDs sparkling at random') ]
];
const USES_WIDTH = [ 'sweep', 'comet', 'converge', 'wave' ];

// How the primary turns into the secondary (the strip crate's Gradient).
const GRADIENTS: readonly [ name: string, label: string ][] = [
	[ 'hue', _('Through the hues: stays vivid (blue to yellow passes cyan and green)') ],
	[ 'mix', _('Straight mix: blue to yellow passes grey') ]
];

// How the pattern meets the background (the strip crate's Edge); breathe and
// heartbeat light the whole strip at once, so they have none.
const EDGES: readonly [ name: string, label: string ][] = [
	[ 'soft', _('Soft: fades into the background') ],
	[ 'solid', _('Solid: fully lit, crisp boundary, colours at full strength') ]
];
const HAS_EDGE = [ 'sweep', 'comet', 'converge', 'wave', 'twinkle' ];

// The dropdowns: their options, and what an empty choice means.
const CHOICES: Record<string, { options: typeof PATTERNS, empty: string }> = {
	pattern: { options: PATTERNS, empty: _('Default (sweep)') },
	gradient: { options: GRADIENTS, empty: _('Default (through the hues)') },
	edge: { options: EDGES, empty: _('Default (soft)') }
};

// An addressable LED strip on a board.
const STRIP: readonly Field[] = [
	[ 'leds', _('LEDs on the strip'), 'range(0,300)' ],
	[ 'pattern', _('Pattern'), 'choice' ],
	[ 'primary', _('Primary colour'), 'color',
		_('The pattern at its head, centre or crest, as #rrggbb or #rgb. Empty means the default blue, #0040ff.') ],
	[ 'secondary', _('Secondary colour'), 'color',
		_('Where the pattern\'s gradient ends, as #rrggbb or #rgb: a comet\'s tail, a glow\'s edges, the far end of the strip. Empty keeps the whole pattern in the primary.') ],
	[ 'gradient', _('Gradient'), 'choice' ],
	[ 'balance', _('Balance'), 'range(0,100)',
		_('How much of the pattern is primary: where along it the gradient is half way. 50 is the middle; higher keeps more of it primary.') ],
	[ 'sharpness', _('Sharpness'), 'range(0,100)',
		_('How abrupt the change is: 0 blends smoothly over the whole pattern, 100 makes a hard edge between the two colours.') ],
	[ 'edge', _('Edge'), 'choice' ],
	[ 'background', _('Background colour'), 'color',
		_('The LEDs outside the pattern, as #rrggbb or #rgb. Empty means black (off).') ],
	[ 'speed', _('Speed'), 'range(1,100)' ],
	[ 'width', _('Width (LEDs)'), 'range(1,50)' ],
	[ 'identify', _('Identify LEDs'), 'flag',
		_('Show a counting pattern instead of the status: the first LED green, every 10th red, the rest dim blue. Count them, enter the number above, then turn this off.') ]
];

// Nested objects in a board or LCD entry and their keys.
const GROUPS = { strip: STRIP, wisps: WISPS };
type Group = keyof typeof GROUPS;
const GROUP_NAMES = Object.keys(GROUPS) as Group[];

// Espressif (vendor 303a) boards currently on USB.
function presentBoards(): Promise<Board[]> {
	return L.resolveDefault(fs.list(USB), []).then((entries: LuCI.fs.FileStatEntry[]) => Promise.all(
		entries.filter((e) => /^\d+-[\d.]+$/.test(e.name)).map((e) => {
			const d = `${USB}/${e.name}`;
			return Promise.all([
				L.resolveDefault(fs.read(`${d}/idVendor`), ''),
				L.resolveDefault(fs.read(`${d}/serial`), ''),
				L.resolveDefault(fs.read(`${d}/product`), '')
			]).then(([ vid, serial, product ]: string[]): Board | null =>
				(vid.trim() == '303a' && serial.trim()) ? { serial: serial.trim(), product: product.trim() } : null);
		})
	)).then((boards) => boards.filter((b): b is Board => b != null));
}

// { brightness, wisps: { sway } } -> { brightness, wisps_sway } for the form.
// Booleans become the '1'/'0' a Flag field uses.
function flatten(entry: unknown): FormSection {
	const out: FormSection = {};
	if (!L.isObject(entry))
		return out;
	const obj = entry as Record<string, unknown>;
	for (const k in obj)
		if (!(k in GROUPS) && obj[k] != null)
			out[k] = obj[k];
	for (const g of GROUP_NAMES) {
		const group = obj[g];
		if (!L.isObject(group))
			continue;
		for (const [ k ] of GROUPS[g]) {
			const v = (group as Record<string, unknown>)[k];
			if (v != null)
				out[`${g}_${k}`] = (typeof v == 'boolean') ? (v ? '1' : '0') : v;
		}
	}
	return out;
}

// The reverse, from a form section; empty fields are left out so they use the
// built-in defaults. Returns null when nothing is set.
function collect(section: FormSection): Entry | null {
	const out: Entry = {};
	const num = (v: unknown): number | null => (v != null && v !== '') ? parseInt(String(v), 10) : null;

	if (section.name != null && section.name !== '')
		out.name = String(section.name);
	const brightness = num(section.brightness);
	if (brightness != null)
		out.brightness = brightness;
	for (const g of GROUP_NAMES) {
		const group: Record<string, number | boolean | string> = {};
		for (const [ k, , datatype ] of GROUPS[g]) {
			const v = section[`${g}_${k}`];
			// A switch is only written while on, and a choice only when one is
			// made, so the file stays minimal.
			if (datatype == 'flag') {
				if (v == '1')
					group[k] = true;
			}
			else if (datatype == 'choice' || datatype == 'color') {
				const text = String(v ?? '').trim();
				if (text)
					group[k] = text;
			}
			else {
				const n = num(v);
				if (n != null)
					group[k] = n;
			}
		}
		if (Object.keys(group).length)
			Object.assign(out, { [g]: group });
	}

	return Object.keys(out).length ? out : null;
}

/** `#rrggbb` or `#rgb`, expanded to `#rrggbb` (the colour picker needs six
 * digits); null if `v` is neither. */
function longHex(v: string): string | null {
	const m = /^#([0-9a-f]{3}|[0-9a-f]{6})$/i.exec(v.trim());
	if (!m)
		return null;
	const hex = m[1].length == 3 ? m[1].replace(/./g, (c) => c + c) : m[1];
	return `#${hex.toLowerCase()}`;
}

// A colour field: LuCI has no colour widget, so this is the usual text box
// (typed hex, validated, empty meaning the default) with the browser's colour
// picker beside it, kept in sync both ways. An empty field shows its default
// colour faded.
const ColorValue = form.Value.extend({
	renderWidget(section_id: string, option_index: number, cfgvalue: unknown) {
		const node = this.super('renderWidget', [ section_id, option_index, cfgvalue ]) as HTMLElement;
		const text = node.querySelector('input') as HTMLInputElement;
		const fallback = longHex(String(this.placeholder ?? '')) ?? '#000000';
		const swatch = E('input', {
			type: 'color',
			title: _('Pick a colour'),
			style: 'width:2.6em; height:2.2em; padding:0; border:0; background:none; cursor:pointer; flex:none'
		}) as HTMLInputElement;

		const show = () => {
			const c = longHex(text.value);
			swatch.value = c ?? fallback;
			swatch.style.opacity = c ? '1' : '0.4';
		};
		swatch.addEventListener('input', () => {
			text.value = swatch.value;
			// The events LuCI's text field listens to, so it revalidates and
			// the form sees the change.
			text.dispatchEvent(new Event('keyup'));
			text.dispatchEvent(new Event('change', { bubbles: true }));
			show();
		});
		text.addEventListener('input', show);

		node.style.display = 'flex';
		node.style.alignItems = 'center';
		node.style.gap = '.5em';
		node.appendChild(swatch);
		show();
		return node;
	}
});

// Live previews: the flame and strip crates compiled to WebAssembly
// (firmware/web), so they run the code the boards run. They follow the form's
// current, unsaved values.
const PREVIEW_WASM = 'status-light/preview.wasm';

/** firmware/web's exports; a number below zero means the default. */
interface PreviewExports {
	memory: WebAssembly.Memory;
	wisps_new(w: number, h: number): number;
	wisps_params(strands: number, height: number, sway: number, speed: number, glow: number, width: number): void;
	wisps_frame(dt_ms: number): number;
	strip_params(pattern: number, speed: number, width: number, primary: number, secondary: number,
		gradient: number, balance: number, sharpness: number, edge: number, background: number): void;
	strip_frame(leds: number, ms: number): number;
}

let previewModule: Promise<WebAssembly.Module> | null = null;

// Compiled once; each preview gets its own instance, so its own animation.
function loadPreview(): Promise<PreviewExports> {
	previewModule ??= fetch(L.resource(PREVIEW_WASM))
		.then((r) => r.ok ? r.arrayBuffer() : Promise.reject(new Error(r.statusText)))
		.then((bytes) => WebAssembly.compile(bytes));
	return previewModule.then((mod) => new WebAssembly.Instance(mod, {}).exports as unknown as PreviewExports);
}

// The strip as a row of LEDs, `leds` across.
function drawStrip(ctx: CanvasRenderingContext2D, rgb: Uint8Array, leds: number) {
	const { width, height } = ctx.canvas;
	ctx.fillStyle = '#000';
	ctx.fillRect(0, 0, width, height);
	const step = width / Math.max(leds, 1);
	const r = Math.max(1, Math.min(step * 0.38, height * 0.3));
	for (let i = 0; i < leds; i++) {
		const [ red, green, blue ] = [ rgb[3 * i], rgb[3 * i + 1], rgb[3 * i + 2] ];
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

// A preview field: a canvas animating `group` for one section, `w` by `h`
// pixels for the flame (the display's size) or a row of LEDs for the strip.
function previewValue(group: Group, w: number, h: number) {
	return form.DummyValue.extend({
		renderWidget(section_id: string) {
			const section = this.section;
			const canvas = E('canvas', {
				width: w,
				height: h,
				style: `width:${w}px; max-width:100%; background:#000; border-radius:4px`
			}) as HTMLCanvasElement;
			const node = E('div', {}, [ canvas ]);

			const text = (key: string) => String(section.formvalue(section_id, `${group}_${key}`) ?? '').trim();
			const num = (key: string) => {
				const v = parseInt(text(key), 10);
				return isNaN(v) ? -1 : v;
			};
			const color = (key: string) => {
				const c = longHex(text(key));
				return c ? parseInt(c.slice(1), 16) : -1;
			};
			const choice = (key: string) => CHOICES[key].options.findIndex(([ name ]) => name == text(key));

			loadPreview().then((x) => {
				const ctx = canvas.getContext('2d')!;
				const image = ctx.createImageData(w, h);
				if (group == 'wisps')
					x.wisps_new(w, h);
				let leds = 0;
				let identify = false;
				const read = () => {
					if (group == 'wisps')
						x.wisps_params(num('strands'), num('height'), num('sway'), num('speed'), num('glow'), num('width'));
					else {
						x.strip_params(choice('pattern'), num('speed'), num('width'), color('primary'), color('secondary'),
							choice('gradient'), num('balance'), num('sharpness'), choice('edge'), color('background'));
						leds = Math.min(num('leds') < 0 ? 60 : num('leds'), 300);
						identify = text('identify') == '1';
					}
				};

				const start = performance.now();
				let last = start, lastRead = -Infinity, shown = false;
				const frame = (now: number) => {
					// Stop once the page drops the canvas (a re-render or another page).
					if (canvas.isConnected)
						shown = true;
					else if (shown)
						return;
					requestAnimationFrame(frame);
					const dt = Math.min(now - last, 100);
					last = now;
					// Not drawn while its tab is hidden.
					if (canvas.offsetParent == null)
						return;
					if (now - lastRead > 250) {
						read();
						lastRead = now;
					}
					if (group == 'wisps') {
						image.data.set(new Uint8Array(x.memory.buffer, x.wisps_frame(Math.round(dt)), w * h * 4));
						ctx.putImageData(image, 0, 0);
					}
					else {
						const rgb = new Uint8Array(x.memory.buffer, x.strip_frame(leds, now - start), leds * 3).slice();
						// The counting pattern (the Zero's identify()): first LED
						// green, every 10th red, the rest dim blue.
						if (identify)
							for (let i = 0; i < leds; i++)
								rgb.set(i == 0 ? [ 0, 255, 0 ] : (i + 1) % 10 == 0 ? [ 255, 0, 0 ] : [ 0, 0, 64 ], 3 * i);
						drawStrip(ctx, rgb, leds);
					}
				};
				requestAnimationFrame(frame);
			}).catch((e: Error) => {
				node.replaceChildren(E('em', {}, _('Preview unavailable: %s could not be loaded (%s).').format(PREVIEW_WASM, e.message)));
			});

			return node;
		}
	});
}

// The preview at the top of a tab.
function addPreview(s: Pick<LuCI.form.AbstractSection, 'taboption'>, tab: string, group: Group, w: number, h: number, description: string) {
	s.taboption(tab, previewValue(group, w, h), `_preview_${group}`, _('Preview'), description);
}

// One field per key of GROUPS[group] on the given tab.
function addGroupOptions(s: Pick<LuCI.form.AbstractSection, 'taboption'>, tab: string, group: Group) {
	for (const [ k, label, datatype, description ] of GROUPS[group]) {
		if (datatype == 'flag') {
			const o = s.taboption(tab, form.Flag, `${group}_${k}`, label, description ?? '');
			o.rmempty = false;
			continue;
		}
		if (datatype == 'color') {
			const o = s.taboption(tab, ColorValue, `${group}_${k}`, label, description ?? '');
			o.placeholder = ({ primary: '#0040ff', background: '#000000' } as Record<string, string>)[k] ?? _('none');
			o.validate = (_section_id: string, value: string) =>
				(!value || longHex(value)) ? true : _('Expecting a colour as #rrggbb or #rgb');
			continue;
		}
		if (datatype == 'choice') {
			const o = s.taboption(tab, form.ListValue, `${group}_${k}`, label);
			o.value('', CHOICES[k].empty);
			for (const [ name, text ] of CHOICES[k].options)
				o.value(name, text);
			// A gradient needs a secondary colour to run to.
			if (k == 'gradient')
				o.depends(`${group}_secondary`, /\S/);
			// Only patterns with a spatial edge have an edge to set.
			if (k == 'edge')
				for (const name of [ '', ...HAS_EDGE ])
					o.depends(`${group}_pattern`, name);
			continue;
		}
		const o = s.taboption(tab, form.Value, `${group}_${k}`, label, description ?? '');
		o.datatype = datatype;
		o.placeholder = _('default');
		// Width only means something to some patterns; hide it for the rest.
		if (group == 'strip' && k == 'width')
			for (const name of [ '', ...USES_WIDTH ])
				o.depends(`${group}_pattern`, name);
		// Balance and sharpness shape a gradient, which needs a secondary colour.
		if (group == 'strip' && (k == 'balance' || k == 'sharpness'))
			o.depends(`${group}_secondary`, /\S/);
	}
}

export default view.extend({
	map: null as LuCI.form.JSONMap | null,
	// The file's LCD settings, kept as they are on routers without flame-screen.
	keptLcd: undefined as Entry | undefined,

	load(): Promise<[ string, Board[], boolean ]> {
		return Promise.all([
			L.resolveDefault(fs.read(FILE), '{}'),
			presentBoards(),
			L.resolveDefault(fs.stat(FLAME_SCREEN).then(() => true), false)
		]);
	},

	render([ text, present, hasScreen ]: [ string, Board[], boolean ]) {
		let settings: Settings = {};
		try {
			const parsed: unknown = JSON.parse(text || '{}');
			if (L.isObject(parsed))
				settings = parsed as Settings;
		}
		catch (e) {
			ui.addNotification(null, E('p', _('%s is not valid JSON (%s); saving will replace it.').format(FILE, (e as Error).message)), 'warning');
		}

		// Loaded boards get explicit names: JSONMap's add() names a new section
		// "board<count>", which can collide with an auto-named loaded one and
		// overwrite it (fixed in later LuCI).
		this.keptLcd = hasScreen ? undefined : settings.lcd;
		const data = {
			...(hasScreen ? { lcd: flatten(settings.lcd) } : {}),
			board: Object.entries(L.isObject(settings.boards) ? settings.boards! : {})
				.map(([ serial, b ], i) => Object.assign({ '.name': `b${i}`, serial }, flatten(b)))
		};

		const m = new form.JSONMap(data, _('Status Light'),
			(hasScreen
				? _('Settings for the router LCD and the USB status boards, saved to %s. Empty fields use the built-in defaults.')
				: _('Settings for the USB status boards, saved to %s. Empty fields use the built-in defaults.')).format(FILE));

		if (hasScreen) {
			const lcd = m.section(form.NamedSection, 'lcd', 'lcd', _('Router LCD'),
				_('The flame shown while GL\'s screen UI sleeps.'));
			lcd.tab('general', _('General'));
			lcd.tab('flame', _('Flame'));
			const b = lcd.taboption('general', form.Value, 'brightness', _('Brightness (%)'));
			b.datatype = 'range(5,100)';
			b.placeholder = '80';
			addPreview(lcd, 'flame', 'wisps', 320, 240,
				_('The router LCD\'s flame with the values below, before saving.'));
			addGroupOptions(lcd, 'flame', 'wisps');
		}

		const boards = m.section(form.TypedSection, 'board', _('USB status boards'),
			_('Matched by USB serial number. Boards currently plugged in are offered in the list.'));
		boards.anonymous = true;
		boards.addremove = true;
		boards.tab('general', _('General'));
		boards.tab('flame', _('Flame'));
		boards.tab('strip', _('Strip'), _('An addressable LED strip on the board, such as the ESP32-S3-Zero\'s. While online it shows the pattern below in your colours; otherwise it shows the status colour.'));

		let o = boards.taboption('general', form.Value, 'serial', _('USB serial number'));
		o.rmempty = false;
		for (const b of present)
			o.value(b.serial, `${b.serial} (${b.product})`);
		o.validate = (section_id: string, value: string) => {
			if (!value)
				return _('A serial number is required');
			const dup = m.data.sections('json', 'board').some((sec: FormSection) =>
				sec['.name'] != section_id && String(sec.serial) == value);
			return dup ? _('This board is already listed') : true;
		};

		o = boards.taboption('general', form.Value, 'name', _('Name'));
		o.placeholder = _('e.g. tdisplay');

		o = boards.taboption('general', form.Value, 'brightness', _('Brightness (%)'));
		o.datatype = 'range(0,100)';
		o.placeholder = '100';
		addPreview(boards, 'flame', 'wisps', 320, 170,
			_('The flame with the values below, before saving, at the T-Display\'s size.'));
		addGroupOptions(boards, 'flame', 'wisps');
		addPreview(boards, 'strip', 'strip', 640, 40,
			_('The pattern while online with the values below, before saving, at full brightness.'));
		addGroupOptions(boards, 'strip', 'strip');

		this.map = m;
		return m.render();
	},

	handleSave(ev: Event) {
		const m = this.map!;
		return m.save(() => {
			const config = m.data;
			const out: Settings = { '$schema': SCHEMA };

			// Without flame-screen there is no LCD section: keep the file's as is.
			const lcd = this.keptLcd ?? collect(config.get('json', 'lcd') || {});
			if (lcd)
				out.lcd = lcd;

			const boards: Record<string, Entry> = {};
			for (const sec of config.sections('json', 'board') as FormSection[]) {
				const serial = String(sec.serial || '').trim();
				if (serial)
					boards[serial] = collect(sec) || {};
			}
			if (Object.keys(boards).length)
				out.boards = boards;

			return fs.write(FILE, JSON.stringify(out, null, '\t') + '\n');
		}).then(() => {
			ui.addNotification(null, E('p', _('Saved. The LCD and the boards pick up the change within a few seconds.')), 'info');
		});
	},

	handleSaveApply: null
});
