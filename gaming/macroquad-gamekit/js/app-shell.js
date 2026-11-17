// macroquad-app: what an app (rather than a game) needs from the page, over
// the miniquad plugin bridge. Four services, each a few functions:
//
// - history:  the page's hash as the app's route, pushed and replaced, and a
//             count of the browser's back and forward moves, so the phone's
//             back gesture goes back inside the app instead of leaving it.
// - insets:   the safe area (a notch, a home indicator) and the height the
//             soft keyboard takes, in CSS pixels, which are the app's points.
// - input:    one real <input> or <textarea> laid over the canvas while a
//             field is being edited, so a phone shows its own keyboard,
//             autocorrect, paste and IME. Focused from inside the tap's own
//             handler, because a phone opens its keyboard only for a focus a
//             gesture made.
// - http:     a request with any method, headers and a binary body, whose
//             reply keeps its status, polled by id like every other plugin.
//
// String and buffer arguments arrive as integer handles to sapp_jsutils
// objects: read them with `consume_js_object(handle)`.

(function () {
    function canvasEl() {
        return document.querySelector('#glcanvas') || document.querySelector('canvas');
    }

    // ── History ─────────────────────────────────────────────────────────────
    //
    // The route lives in the hash, so a reload, a shared link and the back
    // button all land on the same screen. `pops` counts the browser's own
    // moves (back, forward, a typed hash); the app compares it with the count
    // it last saw and re-reads the hash when it moved. Pushes and replaces
    // from the app do not count: the app already knows where it went.

    let pops = 0;
    window.addEventListener('popstate', function () { pops += 1; });
    window.addEventListener('hashchange', function () { pops += 1; });

    function app_hash() {
        const h = window.location.hash || '';
        return js_object(h.charAt(0) === '#' ? h.slice(1) : h);
    }

    function hashUrl(hash) {
        return window.location.pathname + window.location.search + '#' + hash;
    }

    function app_history_push(hash_js) {
        const hash = consume_js_object(hash_js);
        try {
            history.pushState(null, '', hashUrl(hash));
        } catch (e) {
            console.warn('app: could not push a route', e);
        }
    }

    function app_history_replace(hash_js) {
        const hash = consume_js_object(hash_js);
        try {
            history.replaceState(null, '', hashUrl(hash));
        } catch (e) {
            console.warn('app: could not replace the route', e);
        }
    }

    function app_history_back() {
        history.back();
    }

    function app_history_pops() {
        return pops;
    }

    // ── Insets ──────────────────────────────────────────────────────────────
    //
    // `env(safe-area-inset-*)` can only be read by CSS, so a probe element
    // carries them as padding and its computed style is read back. The shell
    // sets `viewport-fit=cover`, without which every inset is zero.
    //
    // The keyboard is what the visual viewport has lost at the bottom: a
    // phone shrinks the visual viewport, not the layout one, when its
    // keyboard opens.
    //
    // The top inset is taken only where the page can reach the top of the
    // screen: an installed app (`display-mode` standalone or fullscreen) or a
    // page in fullscreen. In a browser tab the browser's own bar is there, and
    // a correct browser reports 0 anyway. A wallet app's in-app browser on an
    // edge-to-edge Android reports the status bar's height while its own
    // header sits between the status bar and the page, which drew a band of
    // empty space over the top of the page.

    let probe = null;

    function reachesScreenTop() {
        if (document.fullscreenElement || document.webkitFullscreenElement) return true;
        if (!window.matchMedia) return true;
        return window.matchMedia('(display-mode: standalone)').matches
            || window.matchMedia('(display-mode: fullscreen)').matches
            || window.navigator.standalone === true;
    }

    function ensureProbe() {
        if (probe) return probe;
        probe = document.createElement('div');
        probe.style.cssText = 'position:fixed;left:0;top:0;width:0;height:0;visibility:hidden;'
            + 'pointer-events:none;padding:env(safe-area-inset-top) env(safe-area-inset-right) '
            + 'env(safe-area-inset-bottom) env(safe-area-inset-left);';
        document.body.appendChild(probe);
        return probe;
    }

    // 0 top, 1 right, 2 bottom, 3 left, 4 the keyboard.
    function app_inset(side) {
        if (side === 4) {
            const vv = window.visualViewport;
            if (!vv) return 0;
            return Math.max(0, window.innerHeight - vv.height - vv.offsetTop);
        }
        if (side === 0 && !reachesScreenTop()) return 0;
        const s = getComputedStyle(ensureProbe());
        const v = [s.paddingTop, s.paddingRight, s.paddingBottom, s.paddingLeft][side];
        return parseFloat(v) || 0;
    }

    // ── Input ───────────────────────────────────────────────────────────────
    //
    // Each frame the app names the fields it drew (`app_input_zone`, after
    // `app_input_zones_clear`), with each field's text box as fractions of
    // the canvas. Every named field has its own real `<input>` — or a
    // `<textarea>` for a multi-line field — laid over that box, invisible
    // until it has the focus. The reader's tap lands on the element itself, so
    // the browser focuses it and opens the phone's keyboard as it would for
    // any page. Focusing an element from code after a tap on the canvas is
    // not enough: some phone wallets' in-app browsers open the keyboard only
    // for a tap on the input. Each element follows its box every frame, so a
    // page that scrolls carries it along; a field the app stops naming loses
    // its element, and the focus with it.
    //
    // While editing, the element owns the text: the value the app passes is
    // only taken when editing starts, or when the app sets it on purpose
    // (`app_input_set`, a clear button). The app reads the text, the caret and
    // a textarea's own scroll back each frame so its model agrees with what
    // the reader sees.

    let zones = new Map();        // id -> { box, value, placeholder, kind, font, color, multiline, maxLen }
    let nextZones = new Map();
    let focused = -1;             // the id being edited, or -1
    let wanted = -1;              // a field the app asked to focus, when it is drawn
    let submits = 0;              // submit presses, ever
    const elements = new Map();   // id -> { el, multiline }: each named field's element
    let field = null;             // the element edited last (it keeps its caret after the blur)
    let revealCaret = 0;          // frames to keep a new textarea scrolled to its caret
    let clip = null;              // the box the app's visible area clips the field to
    let zoneClip = [0, 0, 1, 1];  // that area, as canvas fractions
    let inputFont = '';           // the app's face, as a CSS family stack

    const FALLBACK_FONT = 'system-ui,-apple-system,"Segoe UI",Roboto,sans-serif';

    function ensureClip() {
        if (clip) return clip;
        // The canvas is the whole surface the app draws, so the field is
        // clipped to it: a field scrolled under the app bar, or off the page,
        // is cut the way the canvas cuts its own drawing instead of floating
        // over the chrome. The wrapper takes no pointer, so taps outside a
        // field reach the canvas; the fields in it take their own.
        clip = document.createElement('div');
        clip.style.cssText = 'position:fixed;display:none;overflow:hidden;'
            + 'pointer-events:none;z-index:10;';
        document.body.appendChild(clip);
        return clip;
    }

    function makeField(id, multiline) {
        const el = document.createElement(multiline ? 'textarea' : 'input');
        el.dataset.field = String(id);
        if (multiline) {
            el.setAttribute('autocomplete', 'off');
            el.setAttribute('wrap', 'soft');
            el.spellcheck = true;
        } else {
            el.type = 'text';
            el.setAttribute('autocomplete', 'off');
            el.setAttribute('spellcheck', 'false');
        }
        // Invisible until it has the focus: the canvas draws the field, and
        // the element is only there to take the tap.
        el.style.cssText = 'position:absolute;display:block;margin:0;padding:0;border:0;'
            + 'outline:none;background:transparent;box-sizing:border-box;'
            + 'pointer-events:auto;opacity:0;-webkit-tap-highlight-color:transparent;'
            + 'font-family:' + FALLBACK_FONT + ';';
        el.addEventListener('focus', function () {
            if (focused === id) return;
            focused = id;
            field = el;
            el.style.opacity = '1';
            try {
                const end = el.value.length;
                el.setSelectionRange(end, end);
            } catch (e) { /* some input modes refuse a selection */ }
            // The face may still be loading; the next placements and the
            // face's load both call `revealEnd`.
            revealCaret = 2;
        });
        // miniquad's page cancels every paste at the window, for its own
        // clipboard. Stopping the event here, before it bubbles there, lets
        // the field take what was pasted.
        el.addEventListener('paste', function (e) { e.stopPropagation(); });
        el.addEventListener('keydown', function (e) {
            if (e.key === 'Enter') {
                // Return is a newline in a textarea; Ctrl/Cmd+Enter submits.
                if (!multiline || e.metaKey || e.ctrlKey) {
                    submits += 1;
                    e.preventDefault();
                    el.blur();
                }
            } else if (e.key === 'Escape') {
                e.preventDefault();
                el.blur();
            }
        });
        el.addEventListener('blur', function () {
            if (focused === id) focused = -1;
            el.style.opacity = '0';
            const c = canvasEl();
            // Back to the canvas, so the app's own keys work again. A phone
            // does not reopen a keyboard for a canvas.
            if (c) c.focus({ preventScroll: true });
        });
        ensureClip().appendChild(el);
        return el;
    }

    // The element for field `id`, made when the field is first named and
    // remade if it changes between one line and several.
    function ensureElement(id, z) {
        let e = elements.get(id);
        if (e && e.multiline === z.multiline) return e;
        if (e) dropElement(id);
        e = { el: makeField(id, z.multiline), multiline: z.multiline, styled: '' };
        elements.set(id, e);
        return e;
    }

    function dropElement(id) {
        const e = elements.get(id);
        if (!e) return;
        if (focused === id) e.el.blur();
        e.el.remove();
        if (field === e.el) field = null;
        elements.delete(id);
    }

    function place(el, box) {
        const c = canvasEl();
        if (!c) return;
        const r = c.getBoundingClientRect();
        // The app says which rect its fields are drawn in (the body, not the
        // window); the input is clipped to it and positioned inside it.
        const cx = r.left + zoneClip[0] * r.width;
        const cy = r.top + zoneClip[1] * r.height;
        const cw = Math.max(0, (zoneClip[2] - zoneClip[0]) * r.width);
        const ch = Math.max(0, (zoneClip[3] - zoneClip[1]) * r.height);
        if (clip) {
            clip.style.left = cx + 'px';
            clip.style.top = cy + 'px';
            clip.style.width = cw + 'px';
            clip.style.height = ch + 'px';
        }
        el.style.left = (box[0] * r.width - (cx - r.left)) + 'px';
        el.style.top = (box[1] * r.height - (cy - r.top)) + 'px';
        el.style.width = ((box[2] - box[0]) * r.width) + 'px';
        el.style.height = ((box[3] - box[1]) * r.height) + 'px';
    }

    // A textarea's own scroll cannot be set until it has the box it was given
    // and the face it draws in has loaded; both this file's callers run again
    // when those settle.
    function revealEnd(el) {
        try {
            el.setSelectionRange(el.selectionStart, el.selectionEnd);
        } catch (e) { /* some input modes refuse a selection */ }
        el.scrollTop = el.scrollHeight;
    }

    function styleFor(el, z) {
        // A phone zooms the page into any field under 16px when it takes the
        // focus, and does not zoom back out.
        el.style.fontSize = Math.max(16, z.font) + 'px';
        el.style.color = z.color;
        el.style.caretColor = z.color;
        el.style.fontFamily = inputFont || FALLBACK_FONT;
        el.placeholder = z.placeholder;
        // 1: search (the keyboard's key says Search), 2: an address or an id
        // (no capitals, no autocorrect).
        el.setAttribute('enterkeyhint', z.kind === 1 ? 'search' : 'done');
        el.setAttribute('autocapitalize', z.kind === 2 ? 'off' : 'sentences');
        el.setAttribute('autocorrect', z.kind === 2 ? 'off' : 'on');
        el.inputMode = z.kind === 1 ? 'search' : 'text';
        if (z.multiline) {
            el.style.whiteSpace = 'pre-wrap';
            el.style.overflowY = 'auto';
            el.style.resize = 'none';
            // Roughly the canvas's line advance; the two faces measure
            // differently, so a capture and a browser can wrap a word apart.
            el.style.lineHeight = '1.45';
            el.spellcheck = z.kind !== 2;
            el.maxLength = z.maxLen > 0 ? z.maxLen : -1;
        }
    }

    // Focus field `id` from code: for the app's own `app_input_focus` (a
    // capture, a screen that opens on its field). A reader's tap needs none of
    // this; it lands on the element.
    function startEditing(id) {
        const e = elements.get(id);
        if (e) e.el.focus({ preventScroll: true });
    }

    function app_input_zones_clear() {
        nextZones = new Map();
    }

    // The rect the app draws its fields in, as canvas fractions: the input is
    // clipped to it, so a field under the app bar does not float over it.
    function app_input_clip(x0, y0, x1, y1) {
        if (x1 > x0 && y1 > y0) zoneClip = [x0, y0, x1, y1];
    }

    function app_input_zone(id, x0, y0, x1, y1, value_js, placeholder_js, kind, font, color_js,
        multiline, maxLen) {
        const value = consume_js_object(value_js);
        const placeholder = consume_js_object(placeholder_js);
        const color = consume_js_object(color_js);
        if (x1 <= x0 || y1 <= y0) return;
        nextZones.set(id, {
            box: [x0, y0, x1, y1], value, placeholder, kind, font, color,
            multiline: !!multiline, maxLen: maxLen | 0,
        });
    }

    // The frame's zones are complete: swap them in, give every field its
    // element on its box, and let the elements of fields that are gone go.
    function app_input_zones_done() {
        zones = nextZones;
        for (const id of [...elements.keys()]) {
            if (!zones.has(id)) dropElement(id);
        }
        if (zones.size > 0) ensureClip().style.display = 'block';
        else if (clip) clip.style.display = 'none';
        for (const [id, z] of zones) {
            const e = ensureElement(id, z);
            const style = [z.font, z.color, z.placeholder, z.kind, z.maxLen, inputFont].join('|');
            if (e.styled !== style) {
                styleFor(e.el, z);
                e.styled = style;
            }
            // While it is edited the element owns the text; otherwise it
            // carries the app's, ready for the tap that focuses it.
            if (focused !== id && e.el.value !== z.value) e.el.value = z.value;
            place(e.el, z.box);
        }
        if (wanted >= 0 && zones.has(wanted)) {
            startEditing(wanted);
            wanted = -1;
        }
        const e = focused >= 0 ? elements.get(focused) : null;
        if (e && revealCaret > 0) {
            revealCaret -= 1;
            if (e.multiline) revealEnd(e.el);
        }
    }

    function app_input_focused() {
        return focused;
    }

    function app_input_value() {
        return js_object(field ? field.value : '');
    }

    function app_input_set(value_js) {
        const value = consume_js_object(value_js);
        if (!field) return;
        field.value = value;
        try {
            const end = field.value.length;
            field.setSelectionRange(end, end);
        } catch (e) { /* some input modes refuse a selection */ }
    }

    function app_input_submits() {
        return submits;
    }

    function app_input_blur() {
        if (field && focused >= 0) field.blur();
    }

    // The caret and the selection's other end in UTF-16 units, and how far a
    // textarea has scrolled inside itself in CSS pixels. The element survives
    // the blur (it is reused or removed on the next focus), so the app can read
    // where the reader left the caret on the frame the focus went.
    function app_input_caret() {
        if (!field) return -1;
        return field.selectionDirection === 'backward' ? field.selectionStart : field.selectionEnd;
    }

    function app_input_anchor() {
        if (!field) return -1;
        return field.selectionDirection === 'backward' ? field.selectionEnd : field.selectionStart;
    }

    function app_input_scroll() {
        return field ? Math.max(0, field.scrollTop) : 0;
    }

    // The app's face, named once: the canvas draws it from bytes the page
    // cannot see, so the page fetches the same file (`mq-bundle --assets`)
    // and draws the input in it. Until it loads, the fallback stack stays.
    function app_input_font(family_js, url_js) {
        const family = consume_js_object(family_js);
        const url = consume_js_object(url_js);
        if (!family || !url || inputFont) return;
        inputFont = '"' + family.replace(/"/g, '') + '", ' + FALLBACK_FONT;
        // The elements restyle on the next frame: the face is part of their style.
        if (typeof FontFace !== 'function') return;
        const face = new FontFace(family, 'url("' + url.replace(/"/g, '%22') + '")');
        face.load().then(function (loaded) {
            document.fonts.add(loaded);
            // The new face re-wraps the text, so a long note focused at its
            // end has to be scrolled again.
            const e = focused >= 0 ? elements.get(focused) : null;
            if (e && e.multiline) revealEnd(e.el);
        }).catch(function () { /* the fallback stack stays */ });
    }

    // Focus a field once the app has drawn it, for a capture or a screen that
    // opens on its first field. A negative id puts the keyboard away.
    function app_input_focus(id) {
        if (id < 0) {
            wanted = -1;
            if (field && focused >= 0) field.blur();
            return;
        }
        wanted = id;
    }

    // ── Page buttons ────────────────────────────────────────────────────────
    //
    // Some actions a browser allows only while it handles the reader's own
    // gesture, and how long "while" lasts differs: Chrome allows the next
    // frame; WebKit and some wallet apps' in-app browsers allow only the
    // gesture's handler. A canvas acts on a tap in its next frame. So a button
    // whose action needs the gesture is a real `<button>`, invisible, laid
    // over the one the canvas draws: the tap lands on it, the browser decides
    // it was pressed, and the action runs inside its click handler on every
    // browser. The app's widgets (macroquad-app's `CopyButton`) own this; the
    // app itself never names a box.
    //
    // Each frame the widgets name their buttons (`app_button`, between
    // `app_buttons_clear` and `app_buttons_done`): an id, the button's box as
    // fractions of the canvas, an action kind and its payload. A button not
    // named in a frame is removed. Each tap's outcome is counted per id, done
    // or refused, and the widget reads the counts back, so it says "copied"
    // only when the browser copied.
    //
    // Kinds: 1 copy the payload to the clipboard.

    const BUTTON_COPY = 1;

    let pageButtons = new Map();    // id -> { el, kind, payload }
    let nextButtons = new Map();    // id -> { box, kind, payload }
    const buttonCounts = new Map(); // id -> { done, refused }

    function countCopy(id, ok) {
        const n = buttonCounts.get(id) || { done: 0, refused: 0 };
        if (ok) n.done += 1; else n.refused += 1;
        buttonCounts.set(id, n);
    }

    // `execCommand('copy')` first: it answers at once, and it works in the
    // in-app browsers whose async clipboard is refused for want of a
    // permission the host app never grants. The async clipboard is the second
    // try, still started inside the handler.
    function copyInGesture(id, text) {
        let ok = false;
        try {
            const area = document.createElement('textarea');
            area.value = text;
            area.readOnly = true; // no keyboard for it
            area.style.cssText = 'position:fixed;left:0;top:0;opacity:0;font-size:16px;';
            document.body.appendChild(area);
            area.focus({ preventScroll: true });
            area.select();
            area.setSelectionRange(0, text.length); // WebKit ignores select()
            ok = document.execCommand('copy');
            document.body.removeChild(area);
        } catch (e) {
            ok = false;
        }
        const c = canvasEl();
        if (c) c.focus({ preventScroll: true });
        if (ok) {
            countCopy(id, true);
        } else if (navigator.clipboard && navigator.clipboard.writeText) {
            navigator.clipboard.writeText(text).then(
                function () { countCopy(id, true); },
                function () { countCopy(id, false); });
        } else {
            countCopy(id, false);
        }
    }

    function makePageButton(id) {
        const el = document.createElement('button');
        el.type = 'button';
        el.dataset.pageButton = String(id);
        el.style.cssText = 'position:fixed;margin:0;padding:0;border:0;background:transparent;'
            + 'color:transparent;opacity:0;cursor:pointer;z-index:11;'
            + '-webkit-tap-highlight-color:transparent;touch-action:manipulation;';
        el.addEventListener('click', function (e) {
            e.preventDefault();
            e.stopPropagation();
            const b = pageButtons.get(id);
            if (b && b.kind === BUTTON_COPY) copyInGesture(id, b.payload);
        });
        document.body.appendChild(el);
        return el;
    }

    function app_buttons_clear() {
        nextButtons = new Map();
    }

    function app_button(id, x0, y0, x1, y1, kind, payload_js) {
        const payload = consume_js_object(payload_js);
        if (x1 <= x0 || y1 <= y0 || kind !== BUTTON_COPY || payload.length === 0) return;
        nextButtons.set(id, { box: [x0, y0, x1, y1], kind: kind, payload: payload });
    }

    function app_buttons_done() {
        for (const [id, b] of pageButtons) {
            if (!nextButtons.has(id)) {
                b.el.remove();
                pageButtons.delete(id);
            }
        }
        const c = canvasEl();
        const r = c ? c.getBoundingClientRect() : null;
        for (const [id, z] of nextButtons) {
            let b = pageButtons.get(id);
            if (!b) {
                b = { el: makePageButton(id) };
                pageButtons.set(id, b);
            }
            b.kind = z.kind;
            b.payload = z.payload;
            b.el.setAttribute('aria-label', 'Copy ' + z.payload);
            if (r) {
                b.el.style.left = (r.left + z.box[0] * r.width) + 'px';
                b.el.style.top = (r.top + z.box[1] * r.height) + 'px';
                b.el.style.width = ((z.box[2] - z.box[0]) * r.width) + 'px';
                b.el.style.height = ((z.box[3] - z.box[1]) * r.height) + 'px';
            }
        }
    }

    // How many of button `id`'s taps the browser acted on, and how many it
    // refused, ever.
    function app_button_done(id) {
        const n = buttonCounts.get(id);
        return n ? n.done : 0;
    }

    function app_button_refused(id) {
        const n = buttonCounts.get(id);
        return n ? n.refused : 0;
    }

    // ── HTTP ────────────────────────────────────────────────────────────────
    //
    // `headers` is "name\nvalue\n" pairs; `body` an empty buffer for none.
    // A reply of any status settles as done, with its status and bytes: a
    // 401 is an answer the app acts on, not a failure. Only no reply at all
    // (offline, DNS, a CORS refusal) settles as an error.

    const requests = new Map();   // id -> { state: 'pending' } | { state: 'done', status, body } | { state: 'err', message }
    let nextRequest = 1;

    function app_http(method_js, url_js, headers_js, body_js) {
        const id = nextRequest++;
        const method = consume_js_object(method_js);
        const url = consume_js_object(url_js);
        const lines = consume_js_object(headers_js).split('\n');
        const body = consume_js_object(body_js);
        const headers = {};
        for (let i = 0; i + 1 < lines.length; i += 2) {
            if (lines[i]) headers[lines[i]] = lines[i + 1];
        }
        requests.set(id, { state: 'pending' });
        const init = { method, headers };
        if (body && body.length > 0 && method !== 'GET' && method !== 'HEAD') init.body = body;
        fetch(url, init)
            .then(async function (r) {
                const bytes = new Uint8Array(await r.arrayBuffer());
                if (requests.has(id)) requests.set(id, { state: 'done', status: r.status, body: bytes });
            })
            .catch(function (e) {
                const message = e && e.message ? e.message : String(e);
                if (requests.has(id)) requests.set(id, { state: 'err', message });
            });
        return id;
    }

    // 0 pending, 1 done (read the status and the body), 2 failed (read the
    // message), 3 no such request.
    function app_http_state(id) {
        const r = requests.get(id);
        if (!r) return 3;
        return r.state === 'pending' ? 0 : r.state === 'done' ? 1 : 2;
    }

    function app_http_status(id) {
        const r = requests.get(id);
        return r && r.state === 'done' ? r.status : 0;
    }

    // The body, handed over once: the request is forgotten after it.
    function app_http_take(id) {
        const r = requests.get(id);
        requests.delete(id);
        return js_object(r && r.state === 'done' ? r.body : new Uint8Array(0));
    }

    function app_http_error(id) {
        const r = requests.get(id);
        requests.delete(id);
        return js_object(r && r.state === 'err' ? r.message : 'unknown request');
    }

    // A request the app dropped before it settled.
    function app_http_forget(id) {
        requests.delete(id);
    }

    function register_plugin(importObject) {
        importObject.env.app_hash = app_hash;
        importObject.env.app_history_push = app_history_push;
        importObject.env.app_history_replace = app_history_replace;
        importObject.env.app_history_back = app_history_back;
        importObject.env.app_history_pops = app_history_pops;
        importObject.env.app_inset = app_inset;
        importObject.env.app_input_zones_clear = app_input_zones_clear;
        importObject.env.app_input_clip = app_input_clip;
        importObject.env.app_input_zone = app_input_zone;
        importObject.env.app_input_zones_done = app_input_zones_done;
        importObject.env.app_input_focused = app_input_focused;
        importObject.env.app_input_value = app_input_value;
        importObject.env.app_input_set = app_input_set;
        importObject.env.app_input_submits = app_input_submits;
        importObject.env.app_input_blur = app_input_blur;
        importObject.env.app_input_focus = app_input_focus;
        importObject.env.app_input_font = app_input_font;
        importObject.env.app_input_caret = app_input_caret;
        importObject.env.app_input_anchor = app_input_anchor;
        importObject.env.app_input_scroll = app_input_scroll;
        importObject.env.app_buttons_clear = app_buttons_clear;
        importObject.env.app_button = app_button;
        importObject.env.app_buttons_done = app_buttons_done;
        importObject.env.app_button_done = app_button_done;
        importObject.env.app_button_refused = app_button_refused;
        importObject.env.app_http = app_http;
        importObject.env.app_http_state = app_http_state;
        importObject.env.app_http_status = app_http_status;
        importObject.env.app_http_take = app_http_take;
        importObject.env.app_http_error = app_http_error;
        importObject.env.app_http_forget = app_http_forget;
    }

    // Bumped with every change to the functions above, with the Rust export
    // beside it, so a page carrying an older copy fails at instantiate rather
    // than calling a function gl.js stubbed out.
    miniquad_add_plugin({ register_plugin, version: 4, name: 'app_shell' });
})();
