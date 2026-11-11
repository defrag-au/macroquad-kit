# macroquad-app catalogue

**Read this before building an app screen.** One line per module, alphabetical,
generated from each module's own `//!` header by `tests/catalog.rs` — so it cannot drift.

Regenerate: `UPDATE_CATALOG=1 cargo test -p macroquad-app --test catalog`

The shell for a macroquad app that is not a game: run it on
`macroquad_gamekit::Stage::Fluid`, lay it out each frame with `shell_layout`, and it
brings what `macroquad-widgets` deliberately refuses — routes, a real text field with
the phone's keyboard, a long clipped grid and a page that scrolls. Stories are in
`macroquad-storybook` under "app shell". MACROQUAD only: no wasm-bindgen.

14 modules.

| module | what it is |
|---|---|
| `app_bar` | `AppBar` — the bar across the top of every screen: a way back, where the reader is, and the screen's few actions |
| `art_grid` | `ArtGrid` — a scrolling grid of any number of cards, of which only the rows in view are ever visited: a collection of ten thousand costs a frame what a dozen does |
| `chips` | `Chip` — a small rounded toggle (a filter, a sort, a trait value), laid out in rows that wrap rather than scroll sideways |
| `glyph` | `Glyph` — the app shell's icons, drawn with lines and shapes so they are crisp at any scale and need no icon font or asset |
| `history` | `Router` — the app's route kept in the page's hash, so a reload, a shared link and the phone's back gesture all land where the reader expects |
| `http` | `http` — a request with any method, headers and body, whose reply keeps its status and its bytes, polled once a frame |
| `input` | `TextField` — a single-line field the reader types into, with the phone's own keyboard: its autocorrect, its paste, its IME and its Search key |
| `insets` | `Insets` — the parts of the screen the app must not put anything under: the notch and status bar, the home indicator, a rounded corner's cut, and the soft keyboard while it is open |
| `layout` | `shell_layout` — the frame every screen of an app shares: the app bar at the top, the navigation (a bar along the bottom on a phone, a rail down the left on anything wider), and the body between them, all inside the safe area |
| `list` | `List` — a scrolling column of fixed-height rows, of which only the rows in view are ever visited: a thousand settings rows cost a frame what a dozen does |
| `load` | `Fetch` and `Load` — a request whose reply is read into the type a screen wants, and that request's whole life as a screen needs to know it: not asked yet, on its way, here, or failed with a reason to show |
| `nav` | `bottom_nav` and `nav_rail` — the app's top-level destinations, always in reach: a bar along the bottom on a phone, a rail down the left on anything wider |
| `page` | `Page` — a screen that scrolls as a whole and is laid out top to bottom: an asset's detail, a settings list, a market summary |
| `toast` | `Toasts` — short messages about what just happened, stacked above the navigation, each leaving on its own |
