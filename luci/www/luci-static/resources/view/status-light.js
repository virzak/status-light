'use strict';
'require view';
'require form';
'require fs';
'require ui';

// Edits /etc/status-light.json (settings.schema.json in the status-light repo)
// with a JSONMap. JSONMap keeps its data in memory and its save() does nothing,
// so handleSave() collects the edited sections, restores numbers (the form
// stores every value as a string) and writes the file itself. netled and
// flame-screen notice the new file within a few seconds.

const FILE = '/etc/status-light.json';
const SCHEMA = 'https://raw.githubusercontent.com/virzak/status-light/master/settings.schema.json';
const USB = '/sys/bus/usb/devices';

// [key, label, validator] for the wisp parameters (WispParams), the flame the
// LCDs show.
const WISPS = [
	[ 'strands', _('Ribbons'), 'range(1,64)' ],
	[ 'height', _('Height (% of the screen)'), 'range(10,100)' ],
	[ 'sway', _('Sway'), 'range(0,100)' ],
	[ 'speed', _('Speed'), 'range(0,100)' ],
	[ 'glow', _('Glow'), 'range(0,100)' ],
	[ 'width', _('Ribbon width'), 'range(0,100)' ]
];

// An addressable LED strip on a board; a validator of 'flag' is an on/off
// switch, stored as a JSON boolean.
const STRIP = [
	[ 'leds', _('LEDs on the strip'), 'range(0,300)' ],
	[ 'speed', _('Sweep speed'), 'range(1,100)' ],
	[ 'width', _('Glow width (LEDs)'), 'range(1,50)' ],
	[ 'identify', _('Identify LEDs'), 'flag',
		_('Show a counting pattern instead of the status: the first LED green, every 10th red, the rest dim blue. Count them, enter the number above, then turn this off.') ]
];

// Espressif (vendor 303a) boards currently on USB: [{ serial, product }].
function presentBoards() {
	return L.resolveDefault(fs.list(USB), []).then((entries) => Promise.all(
		entries.filter((e) => /^\d+-[\d.]+$/.test(e.name)).map((e) => {
			const d = `${USB}/${e.name}`;
			return Promise.all([
				L.resolveDefault(fs.read(`${d}/idVendor`), ''),
				L.resolveDefault(fs.read(`${d}/serial`), ''),
				L.resolveDefault(fs.read(`${d}/product`), '')
			]).then(([ vid, serial, product ]) => (vid.trim() == '303a' && serial.trim())
				? { serial: serial.trim(), product: product.trim() } : null);
		})
	)).then((boards) => boards.filter(Boolean));
}

// Nested objects in a board or LCD entry and their keys.
const GROUPS = { strip: STRIP, wisps: WISPS };

// { brightness, wisps: { sway } } -> { brightness, wisps_sway } for the form.
// Booleans become the '1'/'0' a Flag field uses.
function flatten(obj) {
	const out = {};
	if (L.isObject(obj)) {
		for (const k in obj)
			if (!(k in GROUPS) && obj[k] != null)
				out[k] = obj[k];
		for (const g in GROUPS)
			for (const [ k ] of GROUPS[g])
				if (L.isObject(obj[g]) && obj[g][k] != null)
					out[`${g}_${k}`] = (typeof obj[g][k] == 'boolean') ? (obj[g][k] ? '1' : '0') : obj[g][k];
	}
	return out;
}

// The reverse, from a form section; empty fields are left out so they use the
// built-in defaults. Returns null when nothing is set.
function collect(section) {
	const out = {};
	const num = (v) => (v != null && v !== '') ? parseInt(v, 10) : null;

	if (section.name != null && section.name !== '')
		out.name = String(section.name);
	if (num(section.brightness) != null)
		out.brightness = num(section.brightness);
	for (const g in GROUPS) {
		const group = {};
		for (const [ k, , datatype ] of GROUPS[g]) {
			const v = section[`${g}_${k}`];
			// A switch is only written while on, so the file stays minimal.
			if (datatype == 'flag') {
				if (v == '1')
					group[k] = true;
			}
			else if (num(v) != null) {
				group[k] = num(v);
			}
		}
		if (Object.keys(group).length)
			out[g] = group;
	}

	return Object.keys(out).length ? out : null;
}

// One field per key of GROUPS[group] on the given tab.
function addGroupOptions(s, tab, group) {
	for (const [ k, label, datatype, description ] of GROUPS[group]) {
		if (datatype == 'flag') {
			const o = s.taboption(tab, form.Flag, `${group}_${k}`, label, description);
			o.rmempty = false;
			continue;
		}
		const o = s.taboption(tab, form.Value, `${group}_${k}`, label);
		o.datatype = datatype;
		o.placeholder = _('default');
	}
}

return view.extend({
	load() {
		return Promise.all([
			L.resolveDefault(fs.read(FILE), '{}'),
			presentBoards()
		]);
	},

	render([ text, present ]) {
		let settings;
		try {
			settings = JSON.parse(text || '{}');
		}
		catch (e) {
			ui.addNotification(null, E('p', _('%s is not valid JSON (%s); saving will replace it.').format(FILE, e.message)), 'warning');
		}
		if (!L.isObject(settings))
			settings = {};

		// Loaded boards get explicit names: JSONMap's add() names a new section
		// "board<count>", which can collide with an auto-named loaded one and
		// overwrite it (fixed in later LuCI).
		const data = {
			lcd: flatten(settings.lcd),
			board: Object.entries(L.isObject(settings.boards) ? settings.boards : {})
				.map(([ serial, b ], i) => Object.assign({ '.name': `b${i}`, serial }, flatten(b)))
		};

		const m = new form.JSONMap(data, _('Status Light'),
			_('Settings for the router LCD and the USB status boards, saved to %s. Empty fields use the built-in defaults.').format(FILE));

		let s = m.section(form.NamedSection, 'lcd', 'lcd', _('Router LCD'),
			_('The flame shown while GL\'s screen UI sleeps.'));
		s.tab('general', _('General'));
		s.tab('flame', _('Flame'));
		let o = s.taboption('general', form.Value, 'brightness', _('Brightness (%)'));
		o.datatype = 'range(5,100)';
		o.placeholder = '80';
		addGroupOptions(s, 'flame', 'wisps');

		s = m.section(form.TypedSection, 'board', _('USB status boards'),
			_('Matched by USB serial number. Boards currently plugged in are offered in the list.'));
		s.anonymous = true;
		s.addremove = true;
		s.tab('general', _('General'));
		s.tab('flame', _('Flame'));
		s.tab('strip', _('Strip'), _('An addressable LED strip on the board, such as the ESP32-S3-Zero\'s, which shows a blue glow sweeping along it while online.'));

		o = s.taboption('general', form.Value, 'serial', _('USB serial number'));
		o.rmempty = false;
		for (const b of present)
			o.value(b.serial, `${b.serial} (${b.product})`);
		o.validate = function(section_id, value) {
			if (!value)
				return _('A serial number is required');
			const dup = this.map.data.sections('json', 'board').some((sec) =>
				sec['.name'] != section_id && String(sec.serial) == value);
			return dup ? _('This board is already listed') : true;
		};

		o = s.taboption('general', form.Value, 'name', _('Name'));
		o.placeholder = _('e.g. tdisplay');

		o = s.taboption('general', form.Value, 'brightness', _('Brightness (%)'));
		o.datatype = 'range(0,100)';
		o.placeholder = '100';
		addGroupOptions(s, 'flame', 'wisps');
		addGroupOptions(s, 'strip', 'strip');

		this.map = m;
		return m.render();
	},

	handleSave(ev) {
		return this.map.save(() => {
			const config = this.map.data;
			const out = { '$schema': SCHEMA };

			const lcd = collect(config.get('json', 'lcd') || {});
			if (lcd)
				out.lcd = lcd;

			const boards = {};
			for (const sec of config.sections('json', 'board')) {
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
