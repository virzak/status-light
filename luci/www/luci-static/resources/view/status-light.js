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

// [key, label, validator] for the flame parameters (FlameParams).
const FLAME = [
	[ 'cooling', _('Cooling'), 'range(0,20)' ],
	[ 'drift', _('Drift'), 'range(0,3)' ],
	[ 'flicker', _('Flicker'), 'range(1,64)' ],
	[ 'seed_min', _('Base heat, cool cells'), 'range(0,255)' ],
	[ 'seed_max', _('Base heat, hot cells'), 'range(0,255)' ],
	[ 'blue_full', _('Blue full at heat'), 'range(1,255)' ],
	[ 'green_start', _('Green starts at heat'), 'range(0,254)' ],
	[ 'white_start', _('White tip starts at heat'), 'range(0,254)' ]
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

// { brightness, flame: { cooling } } -> { brightness, flame_cooling } for the form.
function flatten(obj) {
	const out = {};
	if (L.isObject(obj)) {
		for (const k in obj)
			if (k != 'flame' && obj[k] != null)
				out[k] = obj[k];
		for (const [ k ] of FLAME)
			if (L.isObject(obj.flame) && obj.flame[k] != null)
				out[`flame_${k}`] = obj.flame[k];
	}
	return out;
}

// The reverse, from a form section; empty fields are left out so they use the
// built-in defaults. Returns null when nothing is set.
function collect(section) {
	const out = {};
	const flame = {};
	const num = (v) => (v != null && v !== '') ? parseInt(v, 10) : null;

	if (section.name != null && section.name !== '')
		out.name = String(section.name);
	if (num(section.brightness) != null)
		out.brightness = num(section.brightness);
	for (const [ k ] of FLAME)
		if (num(section[`flame_${k}`]) != null)
			flame[k] = num(section[`flame_${k}`]);
	if (Object.keys(flame).length)
		out.flame = flame;

	return Object.keys(out).length ? out : null;
}

function addFlameOptions(s) {
	for (const [ k, label, datatype ] of FLAME) {
		const o = s.taboption('flame', form.Value, `flame_${k}`, label);
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

		const data = {
			lcd: flatten(settings.lcd),
			board: Object.entries(L.isObject(settings.boards) ? settings.boards : {})
				.map(([ serial, b ]) => Object.assign({ serial }, flatten(b)))
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
		addFlameOptions(s);

		s = m.section(form.TypedSection, 'board', _('USB status boards'),
			_('Matched by USB serial number. Boards currently plugged in are offered in the list.'));
		s.anonymous = true;
		s.addremove = true;
		s.tab('general', _('General'));
		s.tab('flame', _('Flame'));

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
		addFlameOptions(s);

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
