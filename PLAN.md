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
- [x] Vytvoření poznámky (Cmd+N)
- [x] Editor: jeden textarea, první řádek = titulek
- [x] Auto-save při každé změně
- [x] Seznam poznámek v list panelu
- [x] Soft-delete s undo

### UI framework
- [x] Třísloupový layout (sidebar, list, content)
- [x] Design tokeny v tokens.css (barvy, typo, rozměry)
- [x] Texty v texts.rs (všechny UI stringy na jednom místě)
- [x] Globální stav přes Dioxus signals (AppState)
- [x] Klávesové zkratky (Ctrl+J/K navigace, Cmd+D trash, Cmd+N nová poznámka)
- [x] Toast systém s undo
- [x] Settings sekce s pravidly klasifikace

---

## Co chybí

### Space (scope/context switching)
- [ ] DB tabulka `space` (id, name, color, last_used, created_at)
- [ ] Sloupec `space_id` na `web_page`, `note`, `note_folder` a `file` (trash items patří do space)
- [ ] Default space "Personal" při prvním spuštění
- [ ] Aktivní space = poslední použitý (ORDER BY last_used DESC, created_at DESC LIMIT 1)
- [ ] Space přepínač v sidebaru (dropdown místo "lore" titulku)
- [ ] Vytvoření nového space
- [ ] Filtrování veškerého obsahu podle aktivního space
- [ ] Správa spaces v Settings (přejmenování, smazání)

### Složky (hierarchický strom)
- [ ] Stromové zobrazení v sidebaru (expand/collapse šipky, odsazení)
- [ ] Počet poznámek u každé složky (jako Apple Notes)
- [ ] Hover → "..." ikona → kontextové menu:
  - Rename Folder
  - Delete Folder
  - New Folder (podsložka)
- [ ] Vytvoření nové složky ("+" u labelu "Folders")
- [ ] Přesunutí poznámky do složky (drag & drop nebo přes menu)
- [ ] Přesunutí složky (drag & drop na jinou složku = vnořit)
- [ ] Inline rename (klik na název po výběru z menu)
- [ ] Smazání složky → poznámky se přesunou do nadřazené (nebo root)

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
- [ ] Hledání v rámci space (aktuální implementace hledá globálně)
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
- [ ] Cross-space tagging (tag viditelný napříč workspacy)
- [ ] Filtrování podle tagů v list panelu

---

## Prioritní pořadí implementace

### Fáze 1: Space + Složky (aktuální)
Space přepínač, stromové složky v sidebaru s kontextovým menu, poznámky do složek.

### Fáze 2: Rich text editor
Trix nebo podobný editor místo plain textarea. URL detekce a propojení s web archivem.

### Fáze 3: Soubory
Upload, náhled, připojení k poznámkám.

### Fáze 4: Pokročilé vyhledávání
Space-scoped search, milli integrace, snippety.

### Fáze 5: Timeline / kalendář
Heatmapa aktivity, filtr podle data.

### Fáze 6: Web + mobile
Responsive layout, WASM build, synchronizace.
