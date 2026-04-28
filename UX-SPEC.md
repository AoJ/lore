# lore — UX Specification

## Vision

Personal knowledge base combining **web archive** (saved pages with snapshots), **document storage** (stored files) and **notes** (freeform text with hierarchy). One app, three content types, unified search. Desktop-first (Dioxus), same codebase targets web and mobile.

Inspired by Apple Notes (fluid, instant-save, 3-column layout) and Trilium (hierarchy, web clipping, fulltext).

---

## Organizační model

### Workspace

Workspace je nejvyšší organizační jednotka. Odděluje od sebe různé kontexty práce — např. "Osobní", "Firma", "Projekt X". Každý workspace má vlastní složky, poznámky a webové stránky. Soubory a pravidla klasifikace jsou sdílené napříč workspacy.

- Aktivní workspace je zobrazen v horní části sidebaru (pod titulkem "lore").
- Přepínání: klik na název workspace → dropdown se seznamem + "New workspace".
- Každý workspace má název a volitelnou barvu/ikonu.
- Webové stránky přidané přes "Add URL" se přiřadí do aktivního workspace.
- Vyhledávání hledá v rámci aktivního workspace (s možností "Search all workspaces").

### Složky

Složky organizují poznámky do stromové hierarchie uvnitř workspace:

- Každý workspace má vlastní strom složek.
- Složky se mohou libovolně vnořovat (neomezená hloubka).
- Složka může obsahovat poznámky a podsložky.
- Kořenová úroveň = poznámky bez složky (zobrazí se v "Notes").
- Složky v sidebaru jsou skládací (expand/collapse šipkou).

### Vztah obsahu k workspace

| Typ obsahu | Patří do workspace | Může být ve složce |
|------------|-------------------|--------------------|
| Poznámka | Ano | Ano (volitelně) |
| Webová stránka | Ano | Ne (řadí se dle klasifikace) |
| Soubor | Sdílený | Může být připojen k poznámce |

---

## Layout

Tři panely vedle sebe, celá výška okna:

- **Sidebar** (~10rem): Workspace přepínač, navigační strom — sekce, složky, systémové položky.
- **List panel** (~16rem): Seznam položek odpovídající výběru v sidebaru. Vždy viditelný.
- **Content panel** (zbytek): Detail vybrané položky z listu. Editovatelný pro poznámky, read-only pro webové stránky a soubory.

---

## Pohledy

### 1. Sidebar

**Cíl:** Uživatel chce přepínat workspace, navigovat mezi typy obsahu a složkami.

**Prvky (shora dolů):**

- **Workspace přepínač** — název aktivního workspace, klik otevře dropdown:
  - Seznam existujících workspaců
  - "New workspace..." položka na konci
  - Aktivní workspace má zvýraznění
- **Sekce:**
  - Webs — webové stránky v aktivním workspace
  - Notes — všechny poznámky v aktivním workspace (bez složky)
  - Files — všechny soubory
  - Search — globální v aktivním workspace vyhledávání
- **Oddělovač** "Folders"
- **Strom složek** — hierarchický, zanořitelný, skládací:
  - Každá složka má šipku pro expand/collapse
  - Zanořené složky odsazené
  - Klik na složku → list panel zobrazí její poznámky
  - Klik na šipku → jen expand/collapse, nezmění list
- **Oddělovač** "System"
  - Trash — koš se smazanými položkami
  - Settings — nastavení a pravidla

**Akce uživatele:**

- Klik na workspace přepínač → dropdown s workspacy
- Klik na sekci → list panel zobrazí odpovídající položky
- Klik na složku → list panel zobrazí poznámky v dané složce
- Pravý klik na složku → kontextové menu (přejmenovat, smazat, nová podsložka)
- Přetažení poznámky na složku → přesun poznámky
- Klik na "+" u "Folders" labelu → vytvoří novou složku

**Chování:**

- Aktivní položka má zvýrazněné pozadí
- Pouze jedna položka může být aktivní
- Trash zobrazuje počet položek jako badge
- Přepnutí workspace změní obsah všech panelů
- Seznam workspace s možností smazání je v Settings jako další položka.

---

### 2. List panel — Webové stránky

**Cíl:** Uživatel prohlíží seznam uložených webů, vybírá stránku k zobrazení.

**Kdy se zobrazí:** Klik na "All Pages" v sidebaru.

**Prvky:**

- **Nadpis** "Pages"
- **Seznam položek**, každá obsahuje:
  - Titulek (tučně, 1 řádek, oříznutý)
  - Doména · status (šedě)
  - Datum přidání (šedě, menší)

**Akce uživatele:**

- Klik na položku → content panel zobrazí detail stránky, položka se zvýrazní v seznamu
- Scroll → pozice se zapamatuje a zachová při návratu zpět
- Klávesy ↑/↓ → pohyb výběru v seznamu
- Enter → otevření vybrané položky
- Backspace → návrat zpět na seznam na původní pozici
- cmd + d → přesun do koše (s undo notifikací)

**Chování:**

- Řazení: nejnovější nahoře (dle data přidání)
- Seznam se automaticky obnovuje každých 5 sekund (worker na pozadí mění status stránek). Obnovení by nemělo změnit či resetovat vybranou položku, pouze aktualizovat zobrazený obsah nebo přidat stránky
- Nové položky se objeví nahoře bez skoku scrollu
- Smazané položky zmizí s plynulým efektem

---

### 3. List panel — Poznámky

**Cíl:** Uživatel prohlíží poznámky ve složce, vybírá poznámku k editaci.

**Kdy se zobrazí:** Klik na "All Notes" nebo na konkrétní složku v sidebaru.

**Prvky:**

- **Nadpis** — název složky nebo "All Notes"
- **Seznam položek**, každá obsahuje:
  - Titulek poznámky (tučně)
  - První řádek obsahu (šedě, 1 řádek, oříznutý)
  - Datum poslední úpravy (šedě, menší)

**Akce uživatele:**

- Klik na položku → content panel zobrazí editor poznámky
- Cmd+N → vytvoří novou poznámku v aktuální složce, objeví se nahoře v seznamu, content panel zobrazí editor s kurzorem v titulku
- Delete/Backspace → přesun do koše (s undo notifikací)

**Chování:**

- Řazení: dle data poslední úpravy, nejnovější nahoře
- Scroll pozice se zachovává při přepínání mezi poznámkami
- Jako titulek poznámky se použije první řádek v poznámce, nezadává se samostatně
- K poznámce může být přiřazen jeden nebo více souborů (přetažením nebo výběrem souborů)

---

### 4. List panel — Soubory

**Cíl:** Uživatel prohlíží uložené soubory/dokumenty.

**Kdy se zobrazí:** Klik na "All Files" v sidebaru.

**Prvky:**

- **Nadpis** "Files"
- **Seznam položek**, každá obsahuje:
  - Název souboru (tučně)
  - Typ souboru · velikost (šedě)
  - Datum přidání (šedě, menší)

**Akce uživatele:**

- Klik na položku → content panel zobrazí náhled souboru (pokud je podporovaný typ) nebo metadata
- Drag & drop souboru do list panelu → uloží soubor
- cmd + d → přesun do koše

---

### 5. List panel — Vyhledávání

**Cíl:** Uživatel hledá napříč všemi typy obsahu (stránky, poznámky, soubory).

**Kdy se zobrazí:** Klik na "Search" v sidebaru nebo Cmd+F.

**Prvky:**

- **Vyhledávací pole** nahoře (auto-focus)
- **Výsledky** seskupené podle typu:
  - Sekce "Notes" s počtem výsledků
  - Sekce "Web Pages" s počtem výsledků
  - Sekce "Files" s počtem výsledků
- Každý výsledek má stejný formát jako v příslušném list panelu
- Jednotlivé sekce je možné sbalit

**Akce uživatele:**

- Psaní → živé vyhledávání od 2 znaků (debounce 200ms)
- Klik na výsledek → content panel zobrazí detail, list panel zůstává ve vyhledávání
- Escape/Backspace → ukončí vyhledávání, vrátí se k předchozímu pohledu
- Smazání textu → zobrazí placeholder "Type to search across pages and notes."

**Chování:**

- Hledá ve fulltextu obsahu stránek, titulcích a tělech poznámek, názvech souborů
- Žádné výsledky → zobrazí "No results for '{dotaz}'."

---

### 6. List panel — Koš

**Cíl:** Uživatel spravuje smazané položky — obnovuje nebo trvale maže.

**Kdy se zobrazí:** Klik na "Trash" v sidebaru.

**Prvky:**

- **Nadpis** "Trash" s počtem položek
- **Tlačítko** "Empty trash" v záhlaví
- **Seznam smazaných položek**, každá obsahuje:
  - Titulek/název
  - Relativní čas smazání ("před 2 hodinami", "včera")
  - Tlačítka "Restore" a "Delete forever"

**Akce uživatele:**

- Restore → položka se vrátí na původní místo, toast "Restored."
- Delete forever → potvrzovací dialog "Permanently delete this item?" → [Cancel] [Delete]
- Empty trash → potvrzovací dialog "Permanently delete N items?" → [Cancel] [Delete all]

**Chování:**

- Položky starší 30 dnů se automaticky trvale smažou při spuštění aplikace nebo při jejím běhu
- Po restore/delete se seznam okamžitě aktualizuje
- Badge v sidebaru se aktualizuje

---

### 7. Content panel — Detail webové stránky

**Cíl:** Uživatel si prohlíží archivovanou webovou stránku a její metadata.

**Kdy se zobrazí:** Výběr stránky v list panelu.

**Prvky (shora dolů):**

- **URL** — klikatelný odkaz, otevře se v externím prohlížeči
- **Metadata řádek** — doména · kategorie · status · datum · velikost obsahu (šedě, oddělené tečkami)
- **Akční tlačítka:**
  - "Open in browser" — otevře URL v systémovém prohlížeči
  - "Delete" — přesune do koše
- **Screenshot** — náhled stránky jako obrázek
  - Výchozí stav: zmenšený thumbnail (max výška 14rem)
  - Klik na thumbnail → rozbalí na plnou velikost
  - Klik znovu → sbalí zpět
- **Content preview** — rozbalovací sekce s plain textem stránky (výchozí: sbalený)

**Akce uživatele:**

- Klik na URL → otevře v prohlížeči
- Klik na screenshot → toggle zvětšení
- Klik na "Open in browser" → otevře v prohlížeči
- Klik na "Delete" → přesun do koše, toast "Moved to trash. Undo" (5 sekund), content panel zobrazí další položku v seznamu
- Klik na "Undo" v toastu → obnoví položku

---

### 8. Content panel — Editor poznámky

**Cíl:** Uživatel píše a upravuje poznámku. Vše se ukládá automaticky a okamžitě

**Kdy se zobrazí:** Výběr poznámky v list panelu nebo vytvoření nové (Cmd+N).

**Prvky (shora dolů):**

- **Titulek** — editovatelné textové pole, velký font. Prázdný titulek zobrazí placeholder "Untitled note". Pokud je titulek prázdný, v list panelu se jako titulek zobrazí první řádek obsahu.
- **Oddělovač**
- **Tělo** — editovatelná textová oblast. Plain text (v budoucnu rich text).
- **Patička** (šedě, malé):
  - "Created: datum · Modified: datum"
  - Cesta ke složce (klikatelná → naviguje v sidebaru)

**Akce uživatele:**

- Psaní do titulku/těla → auto-save po 500ms nečinnosti. Žádné tlačítko "Uložit".
- Cmd+Backspace → přesun do koše (s undo toastem)

**Chování:**

- Nová poznámka: kurzor v titulku, poznámka se ihned objeví v list panelu
- Změna se okamžitě projeví v list panelu (titulek, preview, datum)
- Žádná ztráta dat — auto-save zajistí uložení před jakoukoliv navigací

---

### 9. Content panel — Detail souboru

**Cíl:** Uživatel si prohlíží uložený soubor a jeho metadata.

**Kdy se zobrazí:** Výběr souboru v list panelu.

**Prvky:**

- **Název souboru** — velký font
- **Metadata řádek** — typ · velikost · datum přidání
- **Náhled** — pro podporované typy (obrázky, PDF) inline náhled; pro ostatní ikona typu souboru
- **Akční tlačítka:**
  - "Open" — otevře soubor v systémové aplikaci
  - "Download/Export" — uloží kopii souboru
  - "Delete" — přesun do koše

---

### 10. Content panel — Pravidla klasifikace webů

**Cíl:** Uživatel si prohlíží a spravuje pravidla, podle kterých se nové URL automaticky klasifikují.

**Kdy se zobrazí:** Klik na "Webpage rules" v sidebaru.

**Prvky:**

- **Nadpis** "Classification Rules"
- **Tabulka pravidel:**
  - Sloupce: Pattern, Match type, Category, Note
  - Řádky seřazené dle priority (nejvyšší nahoře)

**Akce uživatele:**

- Prohlížení pravidel (v první verzi read-only, editace přes SQL/CLI)

---

### 11. Content panel — Prázdný stav

**Cíl:** Informovat uživatele co dělat, když není nic vybráno.

**Kdy se zobrazí:** Žádná položka není vybraná v list panelu.

**Prvky:**

- Centrovaný text (šedě): "Select an item to view it here."

Další prázdné stavy:

- All Pages bez stránek: "No pages yet. Paste a URL in the sidebar to get started."
- All Notes bez poznámek: "No notes yet. Press Cmd+N to create one."
- Prázdná složka: "This folder is empty."
- Hledání bez výsledků: "No results for '{dotaz}'."
- Prázdný koš: "Trash is empty."

---

## Přidání URL

**Kde:** Input pole "Add URL" ve spodní části sidebaru.

**Flow:**

1. Uživatel napíše nebo vloží URL
2. Stiskne Enter
3. URL se zvaliduje, klasifikuje pravidly a vloží do DB
4. Input se vyčistí
5. Pod inputem se na 3 sekundy zobrazí zpráva: "[archive] https://..."
6. Pokud je aktivní "All Pages", nová položka se objeví nahoře v seznamu
7. Pokud URL již existuje: zpráva "Already exists"

---

## Navigace a stav

**Back/Forward:** Aplikace udržuje historii navigace. Cmd+[ jde zpět, Cmd+] jde vpřed. Každá změna sidebar sekce nebo výběru položky se zapisuje do historie.

**Scroll pozice:** List panel si pamatuje scroll pozici per-sekce. Přepnutí na jinou sekci a zpět obnoví předchozí pozici. Content panel si pamatuje scroll pozici per-položka.

**Undo:** Akce smazání (přesun do koše) zobrazí toast s možností "Undo" po dobu 5 sekund. Klik na Undo obnoví položku na původní místo.

---

## Klávesové zkratky

| Zkratka | Akce |
|---------|------|
| Cmd+N | Nová poznámka |
| Cmd+F | Přepnout na vyhledávání |
| Cmd+L | Focus na "Add URL" input |
| Cmd+[ | Navigace zpět |
| Cmd+] | Navigace vpřed |
| Cmd+d | Přesun vybrané položky do koše |
| Backspace | Navigace zpět |
| Cmd+Z | Undo poslední akce |
| Escape | Ukončit vyhledávání / zrušit výběr |
| ↑ / ↓ | Pohyb výběru v list panelu |
| Enter | Otevřít vybranou položku |

---

## Vizuální styl

### Barvy

| Token | Hodnota | Použití |
|-------|---------|---------|
| `--color-bg` | `#ffffff` | Pozadí content panelu, inputů |
| `--color-sidebar` | `#f5f5f5` | Pozadí sidebaru |
| `--color-list` | `#ffffff` | Pozadí list panelu |
| `--color-text` | `#1d1d1f` | Hlavní text, titulky |
| `--color-muted` | `#86868b` | Metadata, popisky, placeholdery |
| `--color-accent` | `#007aff` | Linky, aktivní prvky, primární tlačítka |
| `--color-accent-hover` | `#0062cc` | Hover stav akcentu |
| `--color-border` | `#e0e0e0` | Oddělovače panelů, rámečky inputů |
| `--color-border-light` | `#ececec` | Jemné oddělovače mezi položkami v seznamu |
| `--color-hover` | `#f0f0f0` | Hover pozadí položek v seznamu |
| `--color-selected` | `#e8e8ed` | Pozadí vybrané položky v seznamu a sidebaru |
| `--color-danger` | `#ff3b30` | Delete tlačítko, destruktivní akce |
| `--color-danger-hover` | `#d63029` | Hover stav danger |
| `--color-toast-bg` | `#323232` | Pozadí undo toastu |
| `--color-toast-text` | `#ffffff` | Text undo toastu |

### Typografie

| Token | Hodnota | Použití |
|-------|---------|---------|
| `--font-sans` | `-apple-system, BlinkMacSystemFont, "SF Pro Text", "Segoe UI", system-ui, sans-serif` | Veškerý text |
| `--font-mono` | `"SF Mono", "Menlo", "Consolas", monospace` | Content preview, code bloky |
| `--font-size-base` | `1rem` (16px) | Základní velikost textu |
| `--font-size-sm` | `0.8125rem` (13px) | Metadata, sidebar nav, inputy |
| `--font-size-xs` | `0.6875rem` (11px) | Labely sekcí, table headers |
| `--font-size-lg` | `1.25rem` (20px) | Nadpisy sekcí (Pages, Notes, Rules) |
| `--font-size-xl` | `1.5rem` (24px) | Titulek poznámky v editoru |

### Rozměry

| Token | Hodnota | Použití |
|-------|---------|---------|
| `--sidebar-width` | `10rem` | Šířka sidebaru |
| `--list-width` | `16rem` | Šířka list panelu |
| `--radius` | `0.375rem` | Zaoblení rohů (inputy, tlačítka, karty) |
| `--radius-lg` | `0.5rem` | Zaoblení screenshotu, větších prvků |
| `--spacing-xs` | `0.25rem` | Minimální mezery |
| `--spacing-sm` | `0.5rem` | Vnitřní padding prvků |
| `--spacing-md` | `1rem` | Mezery mezi sekcemi |
| `--spacing-lg` | `1.5rem` | Padding content panelu |

### Téma

Pouze světlé téma. Systémový dark mode se ignoruje (`color-scheme: light only`).
