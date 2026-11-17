// macroquad-text's DOM sink — the page draws the app's text.
//
// On by default for a page that includes this plugin; `?text=canvas` turns it
// off, back to macroquad's own glyphs (for comparing, or a browser that
// misbehaves). When on, the Rust side
// (`macroquad_text::dom`) stops rasterising screen text and sends each frame's
// runs here as one string: one run a line, tab-separated fields, positions
// normalised to the canvas:
//
//   left top width height size family rrggbbaa clip text
//
// Occlusion is already applied by then: a run under a sheet's scrim arrives
// dimmed, one under an opaque card or a toast does not arrive, and one partly
// under it arrives clipped (`macroquad_text::dom::cover`). This layer only
// draws what it is sent.
//
// Each run becomes an absolutely positioned span in a layer over the canvas.
// Spans are pooled by index and only the properties that changed are written,
// so a page at rest costs no DOM writes and a scroll costs one transform per
// run. New spans go in through one DocumentFragment, and the browser's own
// widths are read only after every write of the frame (one layout, not one per
// span), at most WIDTH_READS a frame. The layer stays hidden until the fonts
// the app registered have loaded, so text never flashes in a fallback face.
// `?fit=1` scales each span to the width macroquad laid out, so right-
// and centre-aligned text lands where the layout put it.
//
// Instrumentation: `window.__mqtext` holds counts (frames, runs, writes, spans
// created, width ratios, per-frame time), and `window.__frames` counts frames
// for browser-shot's rate measurement.

(function () {
    const params = new URLSearchParams(window.location.search);
    // Settled 2026-11-12: the browser draws the text unless the page opts out.
    const enabled = params.get('text') !== 'canvas';
    const fit = params.get('fit') === '1';
    // Browser widths read a frame, after its writes: enough for a few new lines a frame, while a
    // screen of hundreds spreads its reads over a few frames instead of one long one.
    const WIDTH_READS = 64;
    // Show the layer even if a font never arrives: fallback text beats no text.
    const FONT_GIVE_UP_MS = 2500;

    const stats = {
        enabled,
        fit,
        frames: 0,
        runs: 0,
        lastRuns: 0,
        maxRuns: 0,
        writes: 0,
        lastWrites: 0,
        maxWrites: 0,
        created: 0,
        fontsAsked: 0,
        fonts: 0,
        shownAtFrame: null,
        ms: 0,
        lastMs: 0,
        maxMs: 0,
        // Browser width over macroquad's width, sampled when a span's text or font changes.
        ratios: [],
        // Presses placed last frame, buttons made for them ever, and taps they sent the canvas.
        presses: 0,
        pressesMade: 0,
        pressed: 0,
    };
    window.__mqtext = stats;
    // A frame counter in both modes, for browser-shot's rate and frame waits: the canvas mode's
    // comes from requestAnimationFrame, which is when miniquad draws.
    window.__frames = 0;
    if (!enabled) {
        const tick = function () {
            window.__frames += 1;
            requestAnimationFrame(tick);
        };
        requestAnimationFrame(tick);
    }

    let layer = null;
    let shown = false;
    let startedAt = performance.now();
    const spans = [];
    // Spans whose browser width is still to be read.
    const unmeasured = new Set();

    function canvasEl() {
        return document.querySelector('#glcanvas') || document.querySelector('canvas');
    }

    function ensureLayer() {
        if (layer) return layer;
        layer = document.createElement('div');
        layer.id = 'mqtext';
        const s = layer.style;
        s.position = 'fixed';
        s.overflow = 'hidden';
        s.pointerEvents = 'none';
        s.zIndex = '5';
        s.left = '0px';
        s.top = '0px';
        s.visibility = 'hidden';
        document.body.appendChild(layer);
        return layer;
    }

    function maybeShow() {
        if (shown || !layer) return;
        const loaded = stats.fontsAsked > 0 && stats.fonts >= stats.fontsAsked;
        if (loaded || performance.now() - startedAt > FONT_GIVE_UP_MS) {
            layer.style.visibility = 'visible';
            shown = true;
            stats.shownAtFrame = stats.frames;
        }
    }

    // Write a style property only when it changes, counting the writes.
    function set(span, key, value) {
        if (span._v[key] === value) return;
        span._v[key] = value;
        span.style[key] = value;
        stats.writes += 1;
    }

    function makeSpan() {
        const span = document.createElement('span');
        span._v = {};
        span._text = null;
        span._nw = null;
        span._w = 0;
        const s = span.style;
        s.position = 'absolute';
        s.left = '0px';
        s.top = '0px';
        s.whiteSpace = 'pre';
        s.transformOrigin = '0 0';
        s.fontKerning = 'normal';
        stats.created += 1;
        return span;
    }

    function mqtext_enabled() {
        return enabled ? 1 : 0;
    }

    function mqtext_font(family_js, bytes_js) {
        const family = consume_js_object(family_js);
        const bytes = consume_js_object(bytes_js);
        stats.fontsAsked += 1;
        try {
            const face = new FontFace(family, bytes);
            face.load().then(function (f) {
                document.fonts.add(f);
                stats.fonts += 1;
                maybeShow();
            }, function (e) {
                console.warn('mqtext: a font failed to load', family, e);
            });
        } catch (e) {
            console.warn('mqtext: FontFace refused a font', family, e);
        }
    }

    function transformOf(span, x, y) {
        let t = 'translate(' + x.toFixed(2) + 'px,' + y.toFixed(2) + 'px)';
        if (fit && span._nw > 0 && span._w > 0) t += ' scaleX(' + (span._w / span._nw).toFixed(4) + ')';
        return t;
    }

    // ── Presses ─────────────────────────────────────────────────────────────
    //
    // A `P` line is something the app offers to be pressed, named by the text
    // drawn on it (macroquad-text's `press`). Each becomes a real <button> in a
    // layer of its own, there for a screen reader and the keyboard, never for
    // the finger: `pointer-events: none`, so a tap still lands on the canvas and
    // a drag that starts on a button still scrolls. Activating one (Enter,
    // Space, a screen reader's double tap) taps the canvas at its centre, so
    // the press does exactly what a finger on it would. Its words are flagged
    // `h` in the runs and hidden from the screen reader, which hears the button.
    //
    // Buttons are kept by name (and which of that name), not by index, so the
    // one with focus stays the same button while the page moves under it.

    let pressLayer = null;
    const pressButtons = new Map(); // key -> button
    // Up to this many presses a frame; a page offering more is not a page of buttons.
    const MAX_PRESSES = 200;

    function ensurePressLayer() {
        if (pressLayer) return pressLayer;
        const style = document.createElement('style');
        style.textContent =
            '#mqpress button{position:absolute;left:0;top:0;margin:0;padding:0;border:0;' +
            'background:transparent;color:transparent;font-size:1px;pointer-events:none;' +
            'border-radius:8px;transform-origin:0 0}' +
            '#mqpress button:focus{outline:none}' +
            '#mqpress button:focus-visible{outline:3px solid #ffffff;outline-offset:2px;' +
            'box-shadow:0 0 0 6px rgba(0,0,0,0.6)}';
        document.head.appendChild(style);
        pressLayer = document.createElement('div');
        pressLayer.id = 'mqpress';
        const s = pressLayer.style;
        s.position = 'fixed';
        s.overflow = 'visible';
        s.pointerEvents = 'none';
        s.zIndex = '6';
        s.left = '0px';
        s.top = '0px';
        document.body.appendChild(pressLayer);
        return pressLayer;
    }

    // A tap on the canvas at (x, y) in client pixels: down now, up two frames
    // later, so the app sees a press and its release in separate frames as it
    // would a finger's.
    function tapCanvas(x, y) {
        const c = canvasEl();
        if (!c) return;
        const at = { clientX: x, clientY: y, button: 0, buttons: 1, bubbles: true, cancelable: true };
        c.dispatchEvent(new MouseEvent('mousemove', at));
        c.dispatchEvent(new MouseEvent('mousedown', at));
        requestAnimationFrame(function () {
            requestAnimationFrame(function () {
                c.dispatchEvent(new MouseEvent('mouseup', Object.assign({}, at, { buttons: 0 })));
            });
        });
        stats.pressed += 1;
    }

    function makePressButton() {
        const b = document.createElement('button');
        b.type = 'button';
        b.addEventListener('click', function (e) {
            e.preventDefault();
            const r = b.getBoundingClientRect();
            tapCanvas(r.left + r.width / 2, r.top + r.height / 2);
        });
        return b;
    }

    // Place this frame's presses: `list` is [x, y, w, h, name] in layer pixels.
    function placePresses(list, box) {
        ensurePressLayer();
        const ps = pressLayer.style;
        const lw = box.width + 'px', lh = box.height + 'px', ll = box.left + 'px', lt = box.top + 'px';
        if (ps.width !== lw) ps.width = lw;
        if (ps.height !== lh) ps.height = lh;
        if (ps.left !== ll) ps.left = ll;
        if (ps.top !== lt) ps.top = lt;
        const seen = new Set();
        const count = new Map();
        for (const [x, y, w, h, name] of list.slice(0, MAX_PRESSES)) {
            const k = (count.get(name) || 0) + 1;
            count.set(name, k);
            const key = name + '\u0000' + k;
            seen.add(key);
            let b = pressButtons.get(key);
            if (!b) {
                b = makePressButton();
                b.setAttribute('aria-label', name);
                b._v = {};
                pressButtons.set(key, b);
                pressLayer.appendChild(b);
                stats.pressesMade += 1;
            }
            set(b, 'transform', 'translate(' + x.toFixed(1) + 'px,' + y.toFixed(1) + 'px)');
            set(b, 'width', w.toFixed(1) + 'px');
            set(b, 'height', h.toFixed(1) + 'px');
        }
        for (const [key, b] of pressButtons) {
            if (!seen.has(key)) {
                b.remove();
                pressButtons.delete(key);
            }
        }
        stats.presses = seen.size;
    }

    function mqtext_frame(frame_js) {
        const started = performance.now();
        const frame = consume_js_object(frame_js);
        const writesBefore = stats.writes;
        ensureLayer();
        const canvas = canvasEl();
        const box = canvas ? canvas.getBoundingClientRect() : { left: 0, top: 0, width: innerWidth, height: innerHeight };
        const W = box.width;
        const H = box.height;
        const ls = layer.style;
        const lw = W + 'px', lh = H + 'px', ll = box.left + 'px', lt = box.top + 'px';
        if (ls.width !== lw) ls.width = lw;
        if (ls.height !== lh) ls.height = lh;
        if (ls.left !== ll) ls.left = ll;
        if (ls.top !== lt) ls.top = lt;

        const lines = frame.length ? frame.split('\n') : [];
        let fresh = null;
        let n = 0;
        const presses = [];
        // Writes only: nothing below reads layout until every span is placed.
        for (let i = 0; i < lines.length; i++) {
            const line = lines[i];
            if (!line) continue;
            const f = line.split('\t');
            if (f[0] === 'P') {
                if (f.length >= 6) {
                    presses.push([+f[1] * W, +f[2] * H, +f[3] * W, +f[4] * H, f.slice(5).join('\t')]);
                }
                continue;
            }
            if (f.length < 9) continue;
            const x = +f[0] * W, y = +f[1] * H, w = +f[2] * W, h = +f[3] * H;
            const size = +f[4] * H;
            const family = f[5][0] === 'm' ? 'mq-m' : 'mq-p';
            // Part of a press's name: the screen reader hears the button instead.
            const named = f[5][1] === 'h';
            const color = '#' + f[6];
            const text = f.slice(8).join('\t');
            let span = spans[n];
            if (!span) {
                span = makeSpan();
                spans[n] = span;
                if (!fresh) fresh = document.createDocumentFragment();
                fresh.appendChild(span);
            }
            n += 1;

            set(span, 'display', '');
            set(span, 'font', size.toFixed(2) + 'px ' + family);
            set(span, 'lineHeight', h.toFixed(2) + 'px');
            set(span, 'color', color);
            span._w = w;
            if (span._text !== text || span._nwFont !== span._v.font) {
                if (span._text !== text) {
                    span._text = text;
                    span.textContent = text;
                    stats.writes += 1;
                }
                span._nw = null;
                span._nwFont = span._v.font;
                unmeasured.add(span);
            }
            if (span._named !== named) {
                span._named = named;
                if (named) span.setAttribute('aria-hidden', 'true');
                else span.removeAttribute('aria-hidden');
                stats.writes += 1;
            }
            span._x = x;
            span._y = y;
            set(span, 'transform', transformOf(span, x, y));

            if (f[7] === '-') {
                set(span, 'clipPath', '');
            } else {
                const c = f[7].split(',');
                const cl = +c[0] * W, ct = +c[1] * H, cr = +c[2] * W, cb = +c[3] * H;
                const it = Math.max(0, ct - y), il = Math.max(0, cl - x);
                const ib = Math.max(0, y + h - cb), ir = Math.max(0, x + w - cr);
                set(span, 'clipPath', 'inset(' + it.toFixed(1) + 'px ' + ir.toFixed(1) + 'px ' + ib.toFixed(1) + 'px ' + il.toFixed(1) + 'px)');
            }
        }
        if (fresh) layer.appendChild(fresh);
        for (let i = n; i < spans.length; i++) {
            set(spans[i], 'display', 'none');
            unmeasured.delete(spans[i]);
        }
        placePresses(presses, box);

        // Reads, after every write: one layout for the frame, at most WIDTH_READS spans.
        if (unmeasured.size && (fit || stats.ratios.length < 4000)) {
            let read = 0;
            for (const span of unmeasured) {
                if (read >= WIDTH_READS) break;
                read += 1;
                unmeasured.delete(span);
                span._nw = span.offsetWidth;
                if (span._w > 0 && stats.ratios.length < 4000) stats.ratios.push(span._nw / span._w);
                if (fit) set(span, 'transform', transformOf(span, span._x, span._y));
            }
        } else if (!fit) {
            unmeasured.clear();
        }
        maybeShow();

        stats.frames += 1;
        stats.lastRuns = n;
        stats.runs += n;
        if (n > stats.maxRuns) stats.maxRuns = n;
        stats.lastWrites = stats.writes - writesBefore;
        if (stats.lastWrites > stats.maxWrites) stats.maxWrites = stats.lastWrites;
        stats.bytes = frame.length;
        const ms = performance.now() - started;
        stats.ms += ms;
        stats.lastMs = ms;
        if (ms > stats.maxMs) stats.maxMs = ms;
        window.__frames = stats.frames;
    }

    register_plugin = function (importObject) {
        importObject.env.mqtext_enabled = mqtext_enabled;
        importObject.env.mqtext_font = mqtext_font;
        importObject.env.mqtext_frame = mqtext_frame;
    };

    miniquad_add_plugin({ register_plugin, version: 2, name: 'mqtext' });
})();
