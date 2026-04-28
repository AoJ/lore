# lore — Inspirace a porovnání

## Tolaria (AGPL-3.0, 7 600 ★)

https://github.com/refactoringhq/tolaria


Znalostní nástroj na Tauri + React + Rust. Nejbližší existující projekt k tomu, co stavíme.

**Co má a my ne:**
- BlockNote editor (Notion-style WYSIWYG na ProseMirror) + CodeMirror raw markdown mode
- Filesystem jako zdroj pravdy (plain .md soubory, žádná DB)
- Git integrace (každý vault = git repo, plná historie)
- Wikilinks a vztahy mezi poznámkami přes YAML frontmatter
- Mermaid diagramy
- MCP server pro AI asistenty
- AI chat panel
- "Pulse View" — aktivitní feed v listu poznámek
- Command Palette (Cmd+K)
- Čtyřpanelový layout (sidebar, list, editor, inspector)

**Co má a my taky:**
- Hierarchické složky
- Fulltext vyhledávání
- Desktop app (Tauri vs Dioxus)

**Co my máme a Tolaria ne:**
- Web archivace se screenshoty a headless Chrome
- Spaces (multi-context switching)
- SQLite jako úložiště (rychlejší search, transakce, revize)
- Cookie banner removal
- URL klasifikační pravidla

**Klíčové poučení:**
- BlockNote (ProseMirror + TipTap) je ověřený v produkci s 10 000+ poznámkami
- Dual-mode editor (WYSIWYG + raw markdown) je dobrý escape hatch
- Čtyřpanelový layout (přidaný inspector panel) stojí za zvážení
- AGPL-3.0 licence — nelze přímo používat kód

---

## WYSIWYG Markdown editory — porovnání

### Milkdown (MIT, 11 400 ★) — DOPORUČENO
https://milkdown.dev

ProseMirror + Remark. Plugin-driven. Headless (vlastní styly). Čistý markdown výstup přes Remark serializer. Funguje v Tauri WebView (ověřeno projekty Moraya, Otterly). ~40KB gzipped. Aktivní vývoj.

**Pro lore:**
- MIT licence
- Nejčistší markdown I/O (Remark je standard)
- Plugin architektura — přidáme jen co potřebujeme
- Headless — plná kontrola nad vzhledem
- Funguje bez React runtime (vanilla JS / framework agnostic)

**Nevýhody:**
- Menší komunita než TipTap
- Méně out-of-box UI komponent

### TipTap (MIT core, 28 000 ★)
https://tiptap.dev

ProseMirror wrapper s čistším API. Největší ekosystém. Markdown extension (MarkedJS parser). Schema-based paste sanitization.

**Pro lore:**
- Největší komunita a dokumentace
- Paste sanitization přes ProseMirror schema
- Markdown extension pro clean output

**Nevýhody:**
- Některé features za paywall (AI, komentáře, kolaborace)
- Markdown extension je "early release"

### BlockNote (MPL-2.0, 16 000 ★)
https://github.com/TypeCellOS/BlockNote

Notion-style bloky na ProseMirror + TipTap. Nejlepší out-of-box UX.

**Pro lore:**
- Ověřený v Tolaria (produkce)
- Nejhezčí výchozí UI

**Nevýhody:**
- React-only (Dioxus nemá React runtime)
- MPL-2.0 licence
- Block model nemusí sedět pro lineární markdown poznámky

### OverType (3 600 ★)
https://github.com/panphora/overtype

Invisible textarea overlay — markdown syntax viditelná ale obarvená. Vanilla JS, zero deps.

**Pro lore:**
- Nejjednodušší integrace (vanilla JS)
- Nulové závislosti
- Markdown zůstává plain text

**Nevýhody:**
- Není true WYSIWYG (syntax markers viditelné)
- Omezené pro komplexnější bloky (tabulky, embedy)

### EasyMDE (MIT, 3 000 ★)
CodeMirror 5 + Marked. Live preview, ne WYSIWYG. Jednoduchý, osvědčený.

### Inkdown (AGPL-3.0, 1 200 ★) — NEPOUŽÍVAT
Electron + Slate.js. **Vývoj zastaven.** Nepoužívat jako závislost.

---

## Doporučení pro lore

### Editor: Milkdown

Důvody:
1. **MIT licence** — bez omezení
2. **Clean markdown I/O** — Remark serializer produkuje standardní markdown
3. **Plugin architektura** — přidáme URL indikátory jako plugin
4. **Headless** — plná kontrola nad styly (naše design tokeny)
5. **Framework agnostic** — vanilla JS, funguje v Dioxus WKWebView
6. **Paste sanitization** — ProseMirror schema filtruje nechtěné formáty
7. **Ověřeno v Tauri WebView** — stejný mechanismus jako Dioxus desktop

### Formát: Markdown (ne HTML)

Uživatel chce:
- Čistý obsah bez formátovacího plevele
- Kopírování z Word/web/PDF bez bordelu
- Pár formátovacích pravidel (bold, italic, heading, list, code, link)
- Indexovatelný plain text

Markdown řeší vše:
- Striktně textový formát → skvělé pro FTS indexing
- Paste sanitization: ProseMirror schema propustí jen povolené bloky
- Verze/diff: standardní textový diff
- Export: markdown je univerzální

### URL indikátory: Milkdown plugin

Milkdown plugin pro `<a>` elementy:
- Detekce URL při paste/psaní
- Auto-archivace (zařazení do fronty)
- CSS pseudo-element s barevnou tečkou dle stavu v DB

### Zpětné odkazy: web_page → poznámky

Nová tabulka nebo query: které poznámky odkazují na danou web stránku.
Zobrazení v detailu stránky: "Referenced in: [poznámka 1], [poznámka 2]"

---

## Další inspirace

### Z Tolaria:
- Inspector panel (metadata, vztahy) — zvážit jako 4. panel
- Command Palette (Cmd+K) — rychlá navigace
- Git integrace — export/backup
- MCP server — AI integrace

### Z Apple Notes:
- Fluid přechody mezi poznámkami
- Inline obrázky
- Checklist s progress barem ve složce

### Z Trilium:
- Hierarchické poznámky (poznámka jako potomek jiné poznámky, ne jen ve složce)
- Kalendářní pohled
- Vztahy mezi poznámkami (graph)
