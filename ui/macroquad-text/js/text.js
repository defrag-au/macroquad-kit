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
        // Writes only: nothing below reads layout until every span is placed.
        for (let i = 0; i < lines.length; i++) {
            const line = lines[i];
            if (!line) continue;
            const f = line.split('\t');
            if (f.length < 9) continue;
            const x = +f[0] * W, y = +f[1] * H, w = +f[2] * W, h = +f[3] * H;
            const size = +f[4] * H;
            const family = f[5] === 'm' ? 'mq-m' : 'mq-p';
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

    miniquad_add_plugin({ register_plugin, version: 1, name: 'mqtext' });
})();
