# lore — Plán funkcionality

## Co je hotové

### Web archivace (modul Webs)
- [x] Přidání URL přes CLI a UI (sidebar input)
- [x] Automatická klasifikace URL podle pravidel v DB (domain, suffix, prefix, contains)
- [x] Normalizace URL (odstranění tracking parametrů)
- [x] Headless Chrome worker (chromiumoxide) — stahuje HTML, plain text, screenshot
- [x] HTTP fallback renderer (bez JS, bez screenshotu)
- [x] Cookie/consent banner removal (CSS injection + JS auto-dismiss)
- [x] FTS5 fulltext vyhledávání přes obsah stránek
- [x] Stav stránky: queued → fetching → archived / failed
- [x] Zobrazení chyby a Retry tlačítko u failed stránek
- [x] Soft-delete (trash) s undo toastem
- [x] Screenshot thumbnail s expand/collapse v detailu
- [x] 5s polling pro live aktualizace z workeru

### Poznámky (modul Notes) — základ
- [x] Vytvoření poznámky (Cmd+N, "+" tlačítko v list panelu)
- [x] Editor: jeden textarea, první řádek = titulek
- [x] Auto-save při každé změně
- [x] Seznam poznámek v list panelu
- [x] Soft-delete s undo
- [x] Přiřazení poznámky do složky (při vytvoření v kontextu složky)
- [x] Filtrování poznámek podle složky / root

### Space (scope/context switching)
- [x] DB tabulka `space` (id, name, color, last_used, created_at)
- [x] Sloupec `space_id` na `web_page`, `note`, `note_folder`
- [x] Default space "Personal" při prvním spuštění
- [x] Aktivní space = poslední použitý (ORDER BY last_used DESC, created_at DESC LIMIT 1)
- [x] Space přepínač v sidebaru (dropdown)
- [x] Vytvoření nového space s inline rename
- [x] Filtrování veškerého obsahu podle aktivního space (pages, notes, folders, trash, search)
- [x] Space izolace ověřena integrační testy

### Složky — základ
- [x] DB tabulka `note_folder` s `parent_id` a `space_id`
- [x] Zobrazení v sidebaru (expand/collapse šipky, odsazení)
- [x] Počet poznámek u každé složky
- [x] Vytvoření nové složky ("+" u labelu "Folders") s inline rename
- [x] Smazání složky → poznámky se přesunou do nadřazené (nebo root)
- [x] Rename složky (inline)
- [x] Vnořování složek (parent_id)

### UI framework
- [x] Třísloupový layout (sidebar, list, content)
- [x] Design tokeny v tokens.css (barvy, typo, rozměry)
- [x] Texty v texts.rs (všechny UI stringy na jednom místě)
- [x] Klávesové zkratky v keys.rs
- [x] Globální stav přes Dioxus signals (AppState)
- [x] Toast systém s undo
- [x] Settings sekce s pravidly klasifikace
- [x] Globální DB revize (triggery, counter, UI indikátor)

### Architektura
- [x] Žádný raw SQL ve views — vše přes lore_core::db funkce
- [x] space_id jako parametr v insert_note, insert_folder, list_notes, list_folders
- [x] Indexy na space_id sloupcích
- [x] 48 integračních testů (space izolace, CRUD, trash, revize, rules, URL extraction)
- [x] Centrální DataStore (store.rs) — jediný zdroj pravdy pro UI data
- [x] Jedna revision-based polling smyčka (2s) místo rozptýlených per-component pollings
- [x] Všechny mutace přes DataStore metody s Result error handling
- [x] Okamžité ukládání poznámek (bez JS debounce) — eliminace race conditions

---

## Co chybí

### Složky — pokročilé
- [x] Hover → "..." ikona → kontextové menu (Rename, Delete, New subfolder) — jako Apple Notes
- [x] Vytvoření podsložky (hierarchicky pod existující)
- [x] Přesunutí poznámky do jiné složky (přes "Move to..." menu se stromovou strukturou)
- [x] Sidebar se přepne na cílovou složku po přesunu, poznámka zůstane selected
- [ ] Přesunutí složky pod jinou složku (drag & drop — odloženo, čeká na web verzi)
- [x] Správa spaces v Settings (přejmenování, smazání, statistiky)

### Poznámky — pokročilé
- [x] Milkdown WYSIWYG markdown editor (ProseMirror + Remark)
- [x] Paste sanitization (ProseMirror schema filtruje nechtěné formáty z Word/web/PDF)
- [x] Auto-save okamžitý (bez debounce, přes DataStore)
- [x] Auto-archivace URL z poznámek (detekce markdown linků i bare URL)
- [x] Zpětné odkazy na stránkách ("Referenced in: [poznámka]")
- [x] Centralizovaný DataStore — žádné race conditions při přepínání poznámek
- [x] URL indikátory v editoru (🟢🟡⚪🔴) — CSS pseudo-elementy, DataStore tracking, auto-refresh
- [x] Vkládání obrázků (paste ze schránky → BLOB v DB, inline zobrazení, orphan cleanup)
- [x] Kalendářní pohled / timeline — heatmap 30 dní, klik na den filtruje poznámky/stránky
- [x] Okamžitá navigace — store.navigate() s přímým refresh (žádný polling lag)
- [x] Připojení souborů k poznámce — file picker (+ Attach file) i drag&drop do editoru, soft-delete místo hard-delete při odstranění z těla (30denní bezpečnostní okno)
- [x] Sekce "Attachments" pod poznámkou — výpis aktivních příloh ve stylu Files (ext · name · date · size · checksum)
- [x] Sekce "Removed" pod poznámkou — soft-deleted přílohy s tlačítkem Restore, auto-cleanup po 30 dnech
- [x] File-block render v těle poznámky — `[name](https://lore.local/attachment/N)` zobrazuje se jako šedá full-width karta s 📎 ikonou; klik otevře nativní save dialog. URL prefix migrace ze starého `lore://attachment/N` (Milkdown ho neuznával jako schéma) + regex unescape pro escapované markdown linky.
- [x] Dedup attachmentů per-note: stejný `name + hash` → reuse ID + insert dalšího odkazu, stejný hash + jiný název = nový soubor (renamed verze)
- [x] **JS build setup pro `milkdown.js`** — `crates/lore-ui/js/` s package.json + index.js (plain JS) + build.mjs (esbuild). Make cíle `js-install`, `js-build`, `js-watch`, `js-clean`. README sekce. Bundle output → `assets/milkdown.js` (committed pro repeatable Rust build). Migrace z minified bundle na zdrojový kód, žádné manuální editace minifikátu.
- [x] **Plný Milkdown custom render pro file blok** — implementováno jako **markView na `link` marku** (ne jako vlastní ProseMirror Node, jak původně plánováno). `js/index.js → buildLinkMarkView` renderuje `<a class="file-attachment-block">` s ext badge vlevo, filename (contentDOM, ProseMirror spravuje text), metadata vpravo (date · size · hash přes `setAttachmentMeta`), × tlačítkem pro odpojení (smaže link mark z dokumentu, soubor zůstane). CSS v `assets/editor.css`. Markdown serializace zůstává `[name](https://attachment.lore.invalid/N)` → plain markdown export funguje bez custom node↔markdown serializátoru.

### Soubory (modul Files)
- [x] DB tabulka `file` (id, name, mime_type, size, hash, data BLOB, created_at, deleted_at)
- [x] Upload souborů (file picker, dedup podle name+hash v rámci space)
- [x] Náhled pro obrázky a PDF (data URI inline)
- [x] Soft-delete (trash) + 30denní cleanup
- [x] Edge case: upload souboru, který je v koši → automatický revive (zachová původní ID)
- [ ] Connect-soubor-k-poznámce přes Files sekci (záměrně oddělené od note attachmentů — viz Notes/Attachmenty)
- [ ] Budoucí: podepsané dokumenty, evidence podpisů, verzování

### Vyhledávání — pokročilé
- [ ] Volba "Search all spaces"
- [ ] milli integrace (Meilisearch engine) — typo tolerance, prefix, jazyk
- [ ] Snippet extraction (zvýraznění nalezených termínů ve výsledcích)
- [ ] Filtrování výsledků (typ, datum, složka, stav)

### UI vylepšení
- [ ] Scroll pozice — zachování při navigaci zpět
- [ ] Back/forward navigační historie (Cmd+[, Cmd+])
- [ ] Kontextové menu (pravý klik) na položkách v listu
- [ ] Automatické smazání trash položek starších 30 dní
- [ ] Prázdné stavy — ilustrace nebo ikony místo jen textu

### Web archivace — pokročilé

Rozplánováno do tří fází; každá je samostatně dokončitelná, ale staví na předchozí.
DB schéma se mění v každé fázi (jedna migrace na fázi). Backend trait roste o
~3–5 metod na fázi. UI komponenty `content_page.rs` se postupně rozšiřuje
(nepřepisuje), nakonec se rozdělí do pod-komponent jako `content_note/`.

---

#### Fáze A — Verzování + re-archivace + ruční mazání verzí  ✅

*Společný základ pro vše ostatní: snapshot je first-class entita s ID a metadaty,
worker při každém fetchi zakládá novou verzi (už dnes), UI ji umí vybrat a smazat.*

**Hotovo:** Migrace 0008 (`title`, `content_hash`, `change_summary` + jednorázový hash backfill).
`insert_snapshot` počítá hash a diffuje proti předchozí verzi. Nové `lore-core` funkce
`list_page_versions`, `get_page_version`, `delete_page_version`, `request_reachive`.
Backend trait + obě implementace + HTTP route. `DataStore::reachive_page` + `delete_page_version`.
UI: tlačítko "Re-archive", sekce "Versions" s badges (current, no change, title changed, ±%),
klik na verzi přepne preview/screenshot, × tlačítko smaže verzi (kromě jediné). CSS v `app.css`.

**Schéma (migrace 0008):**
- `web_page_snapshot.content_hash TEXT` — SHA256 plain_textu (diff detekce)
- `web_page_snapshot.title TEXT` — titulek v době fetche (dnes se ukládá jen na `web_page`, takže verze nezachycuje rename)
- `web_page_snapshot.change_summary TEXT` — JSON `{title_changed: bool, size_delta_pct: i32, content_same: bool}`, vyplněn workerem při insertu
- Backfill kód: pro existující snapshoty spočítat `content_hash` z `plain_text`, `title` zkopírovat z `web_page.title`

**lore-core:**
- `db::list_page_versions(page_id) → Vec<SnapshotMeta>` (id, version, fetched_at, title, size_bytes, content_hash, change_summary)
- `db::get_snapshot(snapshot_id) → SnapshotContent` (html_content, plain_text, screenshot)
- `db::delete_snapshot(snapshot_id)` — smaže jednu verzi (s FTS cleanup; první/jedinou verzi nesmazat → error)
- `insert_snapshot`: spočítat hash, načíst předchozí verzi a vyplnit `change_summary`

**Worker:** žádná změna chování — fetch už dnes zakládá novou verzi přes `insert_snapshot`. Jen `insert_snapshot` v `lore-core` dostane víc práce (hash, summary).

**Backend trait + HTTP API:**
- `list_page_versions`, `get_page_version`, `delete_page_version`, `request_reachive` (= dnešní `update_page_status('queued')`, jen explicitní název pro re-archivaci)

**UI (`content_page.rs`):**
- Tlačítko "Re-archive" vedle "Open in browser" — volá `request_reachive`
- Sekce "Versions" pod meta — collapsible seznam verzí: `v3 · 2026-05-21 · 12.4 KB · [titulek se změnil] [-15%]`
- Kliknutí na verzi přepne zobrazení (screenshot, plain_text_preview) na vybranou
- "Delete this version" tlačítko u každé verze (kromě jediné/aktuální)
- Aktuální = nejvyšší `version`; selector se default-uje na aktuální

**Hotová fáze umožní:** vidět celou historii archivace, ručně vyvolat re-fetch, mazat zastaralé snapshoty (např. když web změnil layout a starý snapshot už nemá hodnotu).

---

#### Fáze B — Readability extrakce + Article-first view

*Postavena na Fázi A: každá nová verze už pojme readability HTML/text.
Backfill starých snapshotů (re-extrakce z uloženého raw HTML) jako bonus.*

**Knihovna:** `dom_smoothie` ([Rust port Mozilla Readability](https://github.com/niklak/dom_smoothie), MIT, žádné JS) nebo `readability` ([dtolnay's port](https://github.com/kumabook/readability), MIT). Vybrat při startu fáze — `dom_smoothie` má lepší účet aktivních releases.

**Schéma (migrace 0009):**
- `web_page_snapshot.readability_html TEXT` — vyextrahovaný `<article>` HTML
- `web_page_snapshot.readability_text TEXT` — plain text z readability_html (pro FTS)
- `web_page_snapshot.byline TEXT`, `excerpt TEXT`, `reading_time_sec INT` — metadata vrácená Readability
- FTS preference: pokud `readability_text` non-null, indexovat ten místo `plain_text` (čistší signal)

**Worker (`lore-worker/src/render.rs`):**
- Po `page.content().await` zavolat readability extrakci na HTML
- Při failu (web bez článkové struktury) `readability_*` zůstanou NULL → UI fallback na plain_text
- `RenderedPage` rozšířit o `readability_html: Option<String>`, atd.; `insert_snapshot` ukládá tyto pole

**Backfill:** **NE**. Migrace přidá jen sloupce s NULL — nepouští readability na existujících snapshotech. Důvod: pokud extrakce vrátí prázdno (článek bez článkové struktury), zůstanou NULL navždy a migrace by je při každém startu zkoušela znovu naplnit. Pro existující data: UI fallback na `plain_text`.

**lore-core:**
- `db::SnapshotContent` rozšířit o `readability_*` pole
- Re-index FTS: při insertu/migraci přepnout zdroj plain_text na readability_text když existuje

**Backend trait:** rozšíření existujícího `get_page_version`, nové metody netřeba.

**UI (`content_page.rs`):**
- Pokud snapshot.readability_html existuje → render jako article (default view)
- Header: `byline · reading_time · excerpt` pod titulkem
- "View raw" link/tab přepne na současný plain_text_preview + raw HTML preview
- Tabs nebo segmented control `Article | Raw`; preference per-page persisted v `localStorage`/`SessionStorage` (pro pozdější web; zatím session signal)

**Hotová fáze umožní:** čisté čtení článku jako v Pocket/Instapaper, bez reklam/navigace; FTS hledá v relevantním obsahu místo v UI stringách.

---

#### Fáze C — Export (HTML, Markdown, JSON)

*Postaveno na Fázi A+B: export bere konkrétní verzi (default aktuální), 
využívá readability_html jako čistý zdroj pro Markdown.*

**lore-core (nový modul `lore_core::export`):**
- `export_html(snapshot, page_meta) → String` — self-contained: title, meta, embed base64 screenshot, inline CSS, readability_html jako tělo, raw HTML jako `<details>` na konci
- `export_markdown(snapshot, page_meta) → String` — frontmatter (url, archived_at, title), pak HTML→MD konverze z `readability_html` (preferovaná knihovna: `html2md` nebo `htmd`)
- `export_json(snapshot, page_meta) → String` — strukturovaný DTO (url, title, byline, archived_at, plain_text, readability_text, content_hash, version, screenshot_b64)
- Sdílený `ExportFormat` enum + `export(snapshot, page_meta, format) → (filename, bytes)` factory

**Backend trait + HTTP API:**
- `export_page(page_id, snapshot_id, format) → (filename, Vec<u8>)` — vrací bytes + suggested filename
- Server přidá raw GET endpoint `/api/pages/{page_id}/export/{snapshot_id}?format=html` pro browser download anchor (analog k `/api/files/{id}/raw`)

**UI (`content_page.rs`):**
- "Export" tlačítko → popover/menu s volbami: HTML / Markdown / JSON
- Per-version: export bere aktuálně vybranou verzi
- Desktop: `rfd::AsyncFileDialog::save_file()` + `write(bytes)`
- Web: `<a href="/api/pages/.../export/...?format=X" download>` přes anchor click

**Filename konvence:** `{domain}-{slug-from-title}-{YYYY-MM-DD}-{HHMMSS}.{ext}` (např. `nytimes-com-rise-of-ai-2026-05-21-143052.html`). Čas povinný — testovací fáze produkuje víc verzí denně, samotné datum by způsobilo přepisování při Save dialogu.

**Hotová fáze umožní:** archivovaný obsah dostat ven (do Obsidianu jako .md, do pipeline jako .json, ke sdílení jako self-contained .html).

---

#### Mimo rámec těchto fází (zapsáno pro pozdější vlnu)

- **Auto re-fetch** stránek starších než N dní (vyžaduje cron-style worker mode)
- **Bulk export** všech archivovaných stránek jako ZIP — přidat až s Phase 3+1 tlačítkem v Settings
- **Diff viewer** — side-by-side porovnání dvou verzí (vyžaduje text-diff knihovnu typu `similar`)
- **Auto-spawning workeru** — aktuálně `retry_page` jen nastaví `status='queued'` a spoléhá na ručně puštěný `make worker`. Pro web verzi server bude muset worker spouštět sám (subprocess nebo embedded). Vyřešit až bude bottleneck.

### Platformy
- [ ] Web verze (Dioxus WASM nebo lore-server s vanilla JS frontendem)
- [ ] Mobilní web (responsive layout, 1-panel na úzkém displeji)
- [ ] Synchronizace/replikace (cr-sqlite nebo HTTP sync nebo něco jiného)
- [ ] API pro externí integraci (browser extension, CLI scripting)

### Tagy
- [ ] Free-form tagy na poznámkách a webových stránkách
- [ ] Cross-space tagging (tag viditelný napříč spaces)
- [ ] Filtrování podle tagů v list panelu

### Infrastruktura / Build / DB management
*Vyplynulo z incidentu 2026-04-29, kdy nová verze kódu tiše selhala na startu kvůli rozdílu schématu a aplikace běžela bez dat — chyba byla swallow-nutá.*

- [x] **Verzované DB schéma** — `PRAGMA user_version` + `EXPECTED_VERSION` v `crates/lore-core/src/migrations.rs`. Při startu: pokud `db > expected` odmítne start, pokud `db < expected` aplikuje chybějící migrace v transakcích. Legacy bridge: pre-versioning DB (`user_version=0` + tabulky existují) se stampne na expected.
- [x] **Centrální místo pro migrace** — `crates/lore-core/migrations/NNNN_popis.sql` embedované přes `include_str!`. Dual-path: SQL soubory + Rust `Step::Code` funkce pro migrace co potřebují kód (SHA256 backfill, regex rewrite). `schema.sql` smazán (zdroj pravdy = migrace + lidský `SCHEMA.md`).
- [ ] **Error handling při startu**
  - Selhání `db::open()` musí být vidět: hláška v dialog boxu / error overlay v UI místo prázdného okna
  - Žádné `.ok()` ani `.unwrap_or_default()` na startup-critical operace — místo toho propagovat až do mainu a zobrazit
  - Verze aplikace + commit hash v error reportu, ať uživatel ví co spustil
- [ ] **Log management**
  - `tracing` crate s `tracing-subscriber` (env-filter)
  - Levely: `error`, `warn`, `info` (default), `debug`, `trace`
  - `RUST_LOG=lore=debug` přepíše level
  - Případně overlay v UI s posledními N error/warn hláškami (collapsible panel?)
- [ ] **Makefile jako jediná pravda pro spouštění**
  - Aktuální `make desktop`, `make serve`, `make worker` — udržovat
  - Přidat: `make desktop-release`, `make migrate` (vynutí migrace bez startu UI), `make db-version`, `make logs` (tail logu)
  - `DB ?= $(CURDIR)/db.sqlite` zachovat — dev výchozí, override `DB=` proměnnou
- [ ] **README.md**
  - Přepsat sekci "Build" → "Spuštění a vývoj" s odkazy na Makefile cíle
  - Doplnit "DB & migrace" sekci popisující versioning a kde leží migrační skripty
  - Doplnit "Logování a debug" sekci s `RUST_LOG`, umístěním logu, jak nahlásit chybu
- [ ] **Správa DB spojení** *(zjištěno 2026-05-13)*
  - Současný stav: `data::open_db()` otevírá nové `Connection` při každém volání. Polling smyčka v UI (každé 2s) tak generuje ~60 spojení/min: jedno raw pro `PRAGMA user_version`, druhé přes `db::open()` (PRAGMA + migrační runner + dotaz na `db_revision`). Plus všechny ad-hoc otevření v event handlerech a `use_signal` initech. Funguje, ale není to ideální — zbytečný CPU/syscall overhead, migrační runner se zbytečně spouští při každém dotazu, vytváří se subtle race window mezi otevřením spojení a SELECT (cizí proces může mezitím zvednout verzi).
  - **Návrh 1 — Skip migrate při každém open (nejlevnější):** Rozdělit `db::open()` na `migrate_and_open()` (jednou při startu, ze CLI a workeru) a `open_existing()` (jen `Connection::open` + per-connection PRAGMAs, žádný migration runner, žádný seed). Polling a všechny ad-hoc dotazy používají `open_existing()`. Migrace patří k bootstrap, ne ke každému dotazu. Malá změna (~10 řádků v `db.rs`), velký logický přínos.
  - **Návrh 2 — Cached connection per polling loop:** Držet jeden `Connection` v Dioxus signalu / `RefCell` / `thread_local!`, polling ho používá místo opakovaného otevírání. Komplikace: rusqlite `Connection` není `Send` — bylo by potřeba `tokio::sync::Mutex<Connection>` nebo thread-local. Přínos: zero open/close overhead při polling.
  - **Návrh 3 — Connection pool (`r2d2_sqlite`):** Pool má smysl při paralelních HTTP requestech (`lore-server`), pro single-threadovou desktop polling smyčku je to overkill.
  - **Návrh 4 — Status quo:** SQLite v WAL módu je rychlý na otevření (desítky µs lokálně), zatím se to nikde neprojevuje. Pokud se ukáže že tohle není bottleneck, nedělat nic.
  - **Doporučení:** udělat #1 (skip migrate při open) jako levný win. K #2 sáhnout až když začneme dělat transakce přes víc operací (např. batch import) nebo se ukáže reálné zpomalení. #3 přijde do hry až s `lore-server` paralelizací.

---

## Prioritní pořadí implementace

### Fáze 1: Space + Složky ✅ (základ hotový)
Space přepínač, stromové složky, poznámky do složek, izolace.
Zbývá: kontextové menu na složkách, podsložky přes menu, přesun drag&drop.

### Fáze 2: Rich text editor
Trix nebo podobný editor místo plain textarea. URL detekce a propojení s web archivem.

### Fáze 3: Soubory
Upload, náhled, připojení k poznámkám.

### Fáze 4: Pokročilé vyhledávání
milli integrace, snippety, "search all spaces".

### Fáze 5: Timeline / kalendář
Heatmapa aktivity, filtr podle data.

### Fáze 6: Web + mobile
Responsive layout, WASM build, synchronizace.
