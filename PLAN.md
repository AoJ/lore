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
- [ ] Custom Milkdown block-widget pro file přílohy (zatím vkládáme jen jako markdown link, plný "block/pruh mezi odstavci" widget vyžaduje rebuild milkdown.js bundle)

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
- [ ] Animace (fade-out při smazání, slide transitions)
- [ ] Kontextové menu (pravý klik) na položkách v listu
- [ ] Automatické smazání trash položek starších 30 dní
- [ ] Prázdné stavy — ilustrace nebo ikony místo jen textu

### Web archivace — pokročilé
- [ ] Re-archivace stránky (nová verze, zachování staré)
- [ ] Prohlížení starších verzí
- [ ] Readability extrakce (čistý obsah článku)
- [ ] Export archivovaných stránek

### Platformy
- [ ] Web verze (Dioxus WASM nebo lore-server s vanilla JS frontendem)
- [ ] Mobilní web (responsive layout, 1-panel na úzkém displeji)
- [ ] Synchronizace/replikace (cr-sqlite nebo HTTP sync)
- [ ] API pro externí integraci (browser extension, CLI scripting)

### Tagy
- [ ] Free-form tagy na poznámkách a webových stránkách
- [ ] Cross-space tagging (tag viditelný napříč spaces)
- [ ] Filtrování podle tagů v list panelu

### Infrastruktura / Build / DB management
*Vyplynulo z incidentu 2026-04-29, kdy nová verze kódu tiše selhala na startu kvůli rozdílu schématu a aplikace běžela bez dat — chyba byla swallow-nutá.*

- [ ] **Verzované DB schéma**
  - Sloupec `schema_version` (nebo tabulka `meta`) v DB
  - Aplikace zná `EXPECTED_VERSION` v kódu
  - Při startu: pokud `db.version < expected` → spustit migrace postupně (1→2→3…), commit verze
  - Pokud `db.version > expected` → **odmítnout start** s jasnou hláškou ("DB schéma v{X}, tato verze aplikace zná jen v{Y} — spusť novější aplikaci")
  - Pokud `db.version == expected` → start
- [ ] **Centrální místo pro migrace**
  - Adresář `crates/lore-core/migrations/NNN_popis.sql` (např. `001_initial.sql`, `002_add_attachment_size_hash.sql`)
  - Embed přes `include_dir!` nebo `include_str!` per soubor
  - Linear forward-only — žádné down-migrace zatím
  - `schema.sql` přestane být zdrojem pravdy → bude jen současný snapshot pro fresh install (generovaný / udržovaný)
- [ ] **Error handling při startu**
  - Selhání `db::open()` musí být vidět: hláška v dialog boxu / error overlay v UI místo prázdného okna
  - Žádné `.ok()` ani `.unwrap_or_default()` na startup-critical operace — místo toho propagovat až do mainu a zobrazit
  - Verze aplikace + commit hash v error reportu, ať uživatel ví co spustil
- [ ] **Log management**
  - `tracing` crate s `tracing-subscriber` (env-filter)
  - Levely: `error`, `warn`, `info` (default), `debug`, `trace`
  - File output do `~/Library/Logs/lore/lore.log` s rotací (např. denní)
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
