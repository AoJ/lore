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
- [x] 33 integračních testů (space izolace, CRUD, trash, revize, rules)

---

## Co chybí

### Složky — pokročilé
- [ ] Hover → "..." ikona → kontextové menu (Rename, Delete, New subfolder) — jako Apple Notes
- [ ] Vytvoření podsložky (hierarchicky pod existující)
- [ ] Přesunutí poznámky do jiné složky (drag & drop nebo přes menu)
- [ ] Přesunutí složky pod jinou složku (drag & drop)
- [ ] Správa spaces v Settings (přejmenování, smazání)

### Poznámky — pokročilé
- [ ] Rich text editor (preferovaný Trix nebo contenteditable s toolbar)
- [ ] Vkládání odkazů — automatická detekce URL, propojení s web archivem:
  - 🟢 odkaz uložen lokálně (archivovaná stránka)
  - 🟡 ve frontě k archivaci
  - ⚪ pouze externí odkaz (filtrováno pravidly)
  - 🔴 archivace selhala
- [ ] Vkládání obrázků (uložení do DB jako BLOB)
- [ ] Připojení souborů k poznámce
- [ ] Kalendářní pohled / timeline — heatmapa aktivity ("co jsem dělal v lednu?")

### Soubory (modul Files)
- [ ] DB tabulka `file` (id, name, mime_type, size, data BLOB, created_at)
- [ ] Upload souborů (drag & drop, file picker)
- [ ] Náhled pro obrázky a PDF
- [ ] Připojení souboru k poznámce
- [ ] Budoucí: podepsané dokumenty, evidence podpisů, obnova

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
