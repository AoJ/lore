# lore — Architektura synchronizace frontend ↔ backend

## Současný stav

### Jak to funguje

1. **DB triggery** — každá změna v SQLite (INSERT/UPDATE/DELETE na note, web_page, note_folder, space) automaticky inkrementuje `db_revision` counter
2. **Polling** — komponenty periodicky čtou `get_revision()` a pokud se číslo změnilo, re-fetchnou svá data
3. **Immediate refresh** — některé akce (trash, move, create) volají `bump_refresh()` pro okamžitou aktualizaci

### Problémy

#### 1. Note editor — race condition při přepínání

**Scénář:** Edituju poznámku A → přepnu na B před uložením → data z A se uloží do B (nebo se ztratí).

**Příčina:**
- Editor ukládá přes JS bridge (Milkdown → textarea → Dioxus oninput)
- Save je debounced (2s) — v JS běží setTimeout
- Při přepnutí poznámky se Dioxus komponenta znovu mountne s novým `id`
- Starý setTimeout ale stále běží a zapíše stará data do nového bridge
- oninput handler nové komponenty uloží stará data pod novým ID

**Dopad:** Duplikáty poznámek, přepsaný obsah, ztráta dat.

#### 2. Nekonzistentní refresh timing

| Komponenta | Refresh mechanismus | Latence |
|------------|-------------------|---------|
| Sidebar (složky, counts) | bump_refresh (okamžitý) | 0ms |
| ListPages | bump_refresh + 5s polling | 0-5s |
| ListNotes | bump_refresh + 3s polling | 0-3s |
| ListTrash | jen bump_refresh | 0ms |
| ContentNote/Page | žádný refresh | statický po loadu |

Editor změní data → sidebar se aktualizuje hned → list za 3s → content panel nikdy. Uživatel vidí nekonzistentní stav.

#### 3. Redundantní DB čtení

- `list_folders(space_id)` se volá 3× na jeden refresh (sidebar, list_notes, content_note)
- `load_rules()` se volá při každém save poznámky (auto_archive_urls)
- Backref query v content_page se volá při každém renderu (ne v effectu)

#### 4. Chybějící error handling

- `open_db().unwrap()` — panic při nedostupné DB
- `.ok()` na mutacích — silent failure bez feedbacku uživateli

---

## Navrhovaná architektura

### Princip: Centralizovaný data store

Místo toho, aby každá komponenta měla vlastní `use_signal` + `use_effect` + polling, vytvořím centrální `DataStore` který:

1. Drží aktuální data v signálech (pages, notes, folders, spaces, trash)
2. Má jednu polling smyčku která kontroluje revizi
3. Poskytuje metody pro mutace (save, delete, move) které:
   - Zapíšou do DB
   - Okamžitě aktualizují lokální signály (optimistic update)
   - V případě chyby rollbacknou

### Struktura

```
DataStore (global context, vedle AppState)
├── revision: Signal<i64>
├── pages: Signal<Vec<PageRow>>
├── notes: Signal<Vec<NoteRow>>  
├── folders: Signal<Vec<FolderRow>>
├── spaces: Signal<Vec<SpaceRow>>
├── trash_items: Signal<Vec<TrashItem>>
├── trash_count: Signal<i64>
├── note_counts: Signal<HashMap<i64, i64>>
│
├── poll() → async loop, kontroluje revizi, aktualizuje signály
├── refresh_all() → okamžitý reload vše
│
├── save_note(id, title, body) → DB write + update notes signal
├── create_note(folder_id, space_id) → DB write + prepend to notes
├── trash_note(id) → DB write + remove from notes + add to trash
├── move_note(id, folder_id) → DB write + update notes signal
├── delete_folder(id) → DB write + update folders + move notes
└── ... (další mutace)
```

### Note editor — řešení race condition

**Klíčová změna:** Note ID musí být součástí save operace, ne implicitní z closure.

```
Editor flow:
1. Milkdown markdownUpdated → scheduleSave(markdown, noteId)
2. JS ukládá noteId do closure timeru
3. Při přepnutí: cleanup(oldNoteId) → okamžitý save s SPRÁVNÝM noteId
4. triggerBridgeSave posílá noteId v data atributu
5. Rust oninput čte noteId z bridge, ne z component prop
6. DataStore.save_note(noteId, title, body) → cíleně aktualizuje správnou poznámku
```

**Pravidla:**
- JS VŽDY posílá noteId s markdown
- Rust VŽDY ověřuje noteId před zápisem
- Cleanup VŽDY uloží pending změny PŘEDTÍM než se zničí editor
- Žádný setTimeout nepřežije přepnutí poznámky

### Polling — jedna smyčka

Místo 4 nezávislých polling loops (ListPages 5s, ListNotes 3s, RevisionIndicator 2s, Sidebar refresh_tick):

```
DataStore.poll() — jedna async smyčka:
  loop {
    sleep(2s)
    new_rev = get_revision()
    if new_rev == last_rev: continue
    
    // Refresh jen to, co se změnilo (heuristika)
    refresh_all_for_current_view()
    last_rev = new_rev
  }
```

Komponenty jen čtou z DataStore signálů — žádné vlastní `use_effect` na refresh.

### Error handling

```rust
enum DataResult<T> {
    Ok(T),
    Err(String),  // User-facing error message
}

// Místo:
lore_core::db::trash_note(&conn, id).ok();

// Nově:
match store.trash_note(id) {
    DataResult::Ok(_) => { /* success toast */ },
    DataResult::Err(msg) => { state.show_toast(msg, None); },
}
```

---

## Implementační kroky

### Fáze A: DataStore základ
1. Vytvořit `store.rs` s DataStore struct
2. Přesunout všechny signály z jednotlivých komponent do DataStore
3. Jedna polling smyčka
4. Komponenty čtou z DataStore, nevolají DB přímo

### Fáze B: Mutace přes DataStore
1. Přesunout všechny DB writes do DataStore metod
2. Optimistic updates (aktualizovat signál hned, DB async)
3. Error handling s rollback

### Fáze C: Note editor fix
1. Note ID v JS bridge (init, save, cleanup)
2. DataStore.save_note(noteId, markdown) — cíleně
3. Cleanup při přepnutí — forced save se správným ID
4. Testy na race condition scénáře

### Fáze D: Optimalizace
1. Deduplikace DB čtení (folders, rules)
2. Cache v DataStore
3. Smarter polling (jen pro aktivní view)
