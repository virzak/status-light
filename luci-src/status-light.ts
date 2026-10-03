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
 * switch, stored as a JSON boolean, 'pattern' a choice from PATTERNS, stored as
 * its name, and 'color' a #rrggbb colour, stored as that string. */
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

// An addressable LED strip on a board.
const STRIP: readonly Field[] = [
	[ 'leds', _('LEDs on the strip'), 'range(0,300)' ],
	[ 'pattern', _('Pattern'), 'pattern' ],
	[ 'primary', _('Primary colour'), 'color',
		_('The pattern at its head, centre or crest, as #rrggbb. Empty means the default blue, #0040ff.') ],
	[ 'secondary', _('Secondary colour'), 'color',
		_('Where the pattern\'s gradient ends, as #rrggbb: a comet\'s tail, a glow\'s edges, the far end of the strip. Empty keeps the whole pattern in the primary.') ],
	[ 'background', _('Background colour'), 'color',
		_('The LEDs outside the pattern, as #rrggbb. Empty leaves them a faint glow of the pattern\'s colour.') ],
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
			else if (datatype == 'pattern' || datatype == 'color') {
				if (v)
					group[k] = String(v);
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

// One field per key of GROUPS[group] on the given tab.
function addGroupOptions(s: Pick<LuCI.form.AbstractSection, 'taboption'>, tab: string, group: Group) {
	for (const [ k, label, datatype, description ] of GROUPS[group]) {
		if (datatype == 'flag') {
			const o = s.taboption(tab, form.Flag, `${group}_${k}`, label, description ?? '');
			o.rmempty = false;
			continue;
		}
		if (datatype == 'color') {
			const o = s.taboption(tab, form.Value, `${group}_${k}`, label, description ?? '');
			o.placeholder = k == 'primary' ? '#0040ff' : _('none');
			o.validate = (_section_id: string, value: string) =>
				(!value || /^#[0-9a-fA-F]{6}$/.test(value)) ? true : _('Expecting a colour as #rrggbb');
			continue;
		}
		if (datatype == 'pattern') {
			const o = s.taboption(tab, form.ListValue, `${group}_${k}`, label);
			o.value('', _('Default (sweep)'));
			for (const [ name, text ] of PATTERNS)
				o.value(name, text);
			continue;
		}
		const o = s.taboption(tab, form.Value, `${group}_${k}`, label);
		o.datatype = datatype;
		o.placeholder = _('default');
		// Width only means something to some patterns; hide it for the rest.
		if (group == 'strip' && k == 'width')
			for (const name of [ '', ...USES_WIDTH ])
				o.depends(`${group}_pattern`, name);
	}
}

export default view.extend({
	map: null as LuCI.form.JSONMap | null,

	load(): Promise<[ string, Board[] ]> {
		return Promise.all([
			L.resolveDefault(fs.read(FILE), '{}'),
			presentBoards()
		]);
	},

	render([ text, present ]: [ string, Board[] ]) {
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
		const data = {
			lcd: flatten(settings.lcd),
			board: Object.entries(L.isObject(settings.boards) ? settings.boards! : {})
				.map(([ serial, b ], i) => Object.assign({ '.name': `b${i}`, serial }, flatten(b)))
		};

		const m = new form.JSONMap(data, _('Status Light'),
			_('Settings for the router LCD and the USB status boards, saved to %s. Empty fields use the built-in defaults.').format(FILE));

		const lcd = m.section(form.NamedSection, 'lcd', 'lcd', _('Router LCD'),
			_('The flame shown while GL\'s screen UI sleeps.'));
		lcd.tab('general', _('General'));
		lcd.tab('flame', _('Flame'));
		let o = lcd.taboption('general', form.Value, 'brightness', _('Brightness (%)'));
		o.datatype = 'range(5,100)';
		o.placeholder = '80';
		addGroupOptions(lcd, 'flame', 'wisps');

		const boards = m.section(form.TypedSection, 'board', _('USB status boards'),
			_('Matched by USB serial number. Boards currently plugged in are offered in the list.'));
		boards.anonymous = true;
		boards.addremove = true;
		boards.tab('general', _('General'));
		boards.tab('flame', _('Flame'));
		boards.tab('strip', _('Strip'), _('An addressable LED strip on the board, such as the ESP32-S3-Zero\'s. While online it shows the pattern below in your colours; otherwise it shows the status colour.'));

		o = boards.taboption('general', form.Value, 'serial', _('USB serial number'));
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
		addGroupOptions(boards, 'flame', 'wisps');
		addGroupOptions(boards, 'strip', 'strip');

		this.map = m;
		return m.render();
	},

	handleSave(ev: Event) {
		const m = this.map!;
		return m.save(() => {
			const config = m.data;
			const out: Settings = { '$schema': SCHEMA };

			const lcd = collect(config.get('json', 'lcd') || {});
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
