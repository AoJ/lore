// Milkdown-based markdown editor for the lore desktop app.
//
// Exposes `window.loreEditor` with the API consumed by `content_note.rs`:
//   init(rootId, content, bridgeId, noteId)  -> Promise<editor>
//   cleanup(noteId)
//   destroy()
//   getContent()                              -> markdown string
//   insertImage(name, url)                    -> inserts ![name](url) at cursor
//   insertFile(name, url)                     -> inserts a Link mark at cursor (immediate render)
//   resolveAttachments(map)                   -> swap attachment <img> src to data URIs
//   updateUrlStatuses(map)                    -> apply url-archived/queued/external/failed classes
//   setAttachmentMeta(map)                    -> id → {name,size,hash,created_at,mime_type} for rich block render

import {
  Editor,
  rootCtx,
  defaultValueCtx,
  editorViewCtx,
  editorViewOptionsCtx,
  serializerCtx,
} from '@milkdown/core';
import { commonmark } from '@milkdown/preset-commonmark';
import { gfm } from '@milkdown/preset-gfm';
import { listener, listenerCtx } from '@milkdown/plugin-listener';

const ATTACHMENT_URL_PREFIX = 'https://attachment.lore.invalid/';

let editor = null;
let activeNoteId = null;

// id (string) -> { name, size, hash, created_at, mime_type }
const attachmentMeta = {};

// ---- Helpers ----

function setDirty(dirty) {
  const el = document.getElementById('dirty-indicator');
  if (el) el.style.opacity = dirty ? '1' : '0';
}

function bridgePush(bridgeId, value) {
  const bridge = document.getElementById(bridgeId);
  if (!bridge) return;
  const setter = Object.getOwnPropertyDescriptor(
    window.HTMLTextAreaElement.prototype,
    'value',
  ).set;
  setter.call(bridge, value);
  bridge.dispatchEvent(new Event('input', { bubbles: true }));
}

function pushMarkdownToBridge(markdown, noteId) {
  const bridge = document.getElementById('milkdown-bridge');
  if (!bridge) return;
  bridge.setAttribute('data-note-id', String(noteId));
  bridgePush('milkdown-bridge', markdown);
}

function attachmentIdFromUrl(url) {
  if (!url || !url.startsWith(ATTACHMENT_URL_PREFIX)) return null;
  const m = url.slice(ATTACHMENT_URL_PREFIX.length).match(/^(\d+)/);
  return m ? m[1] : null;
}

function fileExt(name) {
  if (!name) return 'FILE';
  const idx = name.lastIndexOf('.');
  return idx > 0 ? name.slice(idx + 1).toUpperCase() : 'FILE';
}

function formatSize(bytes) {
  const n = Number(bytes) || 0;
  if (n >= 1e9) return (n / 1e9).toFixed(1) + ' GB';
  if (n >= 1e6) return (n / 1e6).toFixed(1) + ' MB';
  if (n >= 1e3) return (n / 1e3).toFixed(1) + ' KB';
  return n + ' B';
}

function shortDate(iso) {
  return iso ? String(iso).slice(0, 10) : '';
}

function escapeHtml(s) {
  return String(s).replace(/[&<>"']/g, c =>
    ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]
  );
}

// ---- Attachment markView ----

function applyMetaToDom(dom) {
  const id = dom.getAttribute('data-att-id');
  if (!id) return;
  const meta = attachmentMeta[id];
  if (!meta) return;
  const badge = dom.querySelector('.file-ext-badge');
  if (badge) badge.textContent = fileExt(meta.name || badge.textContent);
  const slot = dom.querySelector('.file-attachment-meta');
  if (slot) {
    const hashShort = (meta.hash || '').slice(0, 8);
    let html =
      `<span>${escapeHtml(shortDate(meta.created_at))}</span>` +
      `<span class="sep">·</span>` +
      `<span>${escapeHtml(formatSize(meta.size))}</span>`;
    if (hashShort) {
      html +=
        `<span class="sep">·</span>` +
        `<span class="file-hash">${escapeHtml(hashShort)}</span>`;
    }
    slot.innerHTML = html;
  }
}

function refreshAllAttachmentMarkViews() {
  if (!editor) return;
  editor.action(ctx => {
    const view = ctx.get(editorViewCtx);
    view.dom.querySelectorAll('a.file-attachment-block[data-att-id]').forEach(applyMetaToDom);
  });
}

function removeAttachmentLinkAtNameWrap(nameWrap, href) {
  if (!editor) return;
  editor.action(ctx => {
    const view = ctx.get(editorViewCtx);
    const linkType = view.state.schema.marks.link;
    if (!linkType) return;

    let pos;
    try { pos = view.posAtDOM(nameWrap, 0); } catch { return; }
    if (pos == null || pos < 0) return;

    const doc = view.state.doc;

    // Walk backward through contiguous text nodes carrying same href.
    let from = pos;
    while (from > 0) {
      const $p = doc.resolve(from);
      const before = $p.nodeBefore;
      if (!before || !before.isText) break;
      if (!before.marks.some(m => m.type === linkType && m.attrs.href === href)) break;
      from -= before.nodeSize;
    }
    // Walk forward.
    let to = pos;
    while (to < doc.content.size) {
      const $p = doc.resolve(to);
      const after = $p.nodeAfter;
      if (!after || !after.isText) break;
      if (!after.marks.some(m => m.type === linkType && m.attrs.href === href)) break;
      to += after.nodeSize;
    }

    if (from < to) {
      view.dispatch(view.state.tr.delete(from, to));
    }
  });
}

function buildLinkMarkView(mark, _view, _inline) {
  const href = mark.attrs.href;
  const id = attachmentIdFromUrl(href);
  if (!id) {
    // Non-attachment link — render same DOM as Milkdown default would, so
    // CSS rules for `.url-archived` etc. and the URL-indicator dot keep working.
    const a = document.createElement('a');
    if (href) a.setAttribute('href', href);
    if (mark.attrs.title) a.setAttribute('title', mark.attrs.title);
    return { dom: a, contentDOM: a };
  }

  const dom = document.createElement('a');
  dom.className = 'file-attachment-block';
  dom.setAttribute('href', href);
  dom.setAttribute('data-att-id', id);

  // Extension badge — initial guess from the link's title attr.
  const badge = document.createElement('span');
  badge.className = 'file-ext-badge';
  badge.contentEditable = 'false';
  badge.textContent = fileExt(mark.attrs.title || '');
  dom.appendChild(badge);

  // Name slot — this is the contentDOM, ProseMirror manages its text.
  const nameWrap = document.createElement('span');
  nameWrap.className = 'file-attachment-name';
  dom.appendChild(nameWrap);

  // Metadata (date · size · hash) — populated via setAttachmentMeta.
  const metaSlot = document.createElement('span');
  metaSlot.className = 'file-attachment-meta';
  metaSlot.contentEditable = 'false';
  dom.appendChild(metaSlot);

  // Close button — removes the link from the document.
  const close = document.createElement('button');
  close.className = 'file-attachment-close';
  close.type = 'button';
  close.contentEditable = 'false';
  close.title = 'Remove from note';
  close.textContent = '×';
  dom.appendChild(close);

  // Click routing: × → delete, name → text edit (default), elsewhere → download.
  dom.addEventListener('click', (e) => {
    if (close.contains(e.target)) {
      e.preventDefault();
      e.stopPropagation();
      removeAttachmentLinkAtNameWrap(nameWrap, href);
      return;
    }
    if (nameWrap.contains(e.target)) {
      // Let ProseMirror handle text selection / cursor placement inside the name.
      return;
    }
    // Click on badge / meta / outer padding → trigger download dialog.
    e.preventDefault();
    e.stopPropagation();
    bridgePush('att-download-bridge', id);
  }, true);

  applyMetaToDom(dom);

  return { dom, contentDOM: nameWrap };
}

// ---- API ----

window.loreEditor = {
  async init(rootId, content, _bridgeId, noteId) {
    if (editor) {
      try { editor.destroy(); } catch { /* ignore */ }
      editor = null;
    }
    activeNoteId = noteId;
    setDirty(false);

    const root = document.getElementById(rootId);
    if (!root) return null;
    root.innerHTML = '';

    try {
      const created = await Editor.make()
        .config(ctx => {
          ctx.set(rootCtx, root);
          ctx.set(defaultValueCtx, content || '');
          ctx.update(editorViewOptionsCtx, prev => ({
            ...prev,
            markViews: {
              link: buildLinkMarkView,
            },
          }));
        })
        .use(commonmark)
        .use(gfm)
        .use(listener)
        .config(ctx => {
          ctx.get(listenerCtx).markdownUpdated((_c, md, prevMd) => {
            if (md !== prevMd) pushMarkdownToBridge(md, activeNoteId);
          });
        })
        .create();

      editor = created;

      const pm =
        root.querySelector('.ProseMirror') ||
        root.querySelector('[contenteditable]');
      if (pm) {
        pm.setAttribute('spellcheck', 'false');
        pm.setAttribute('autocorrect', 'off');
        pm.setAttribute('autocapitalize', 'off');
        pm.setAttribute('data-gramm', 'false');
        pm.focus();

        // Image paste — JS sends data URI to #image-bridge for Rust to upload.
        pm.addEventListener('paste', ev => {
          const items = ev.clipboardData && ev.clipboardData.items;
          if (!items) return;
          for (const item of items) {
            if (item.type.indexOf('image/') !== 0) continue;
            ev.preventDefault();
            const file = item.getAsFile();
            if (!file) continue;
            const reader = new FileReader();
            reader.onload = e => bridgePush('image-bridge', e.target.result);
            reader.readAsDataURL(file);
            break;
          }
        });
      }

      // attachmentMeta is intentionally NOT cleared here — attachment IDs are
      // globally unique (tied to a single note via FK), so accumulating across
      // note switches is safe and avoids a race where setAttachmentMeta from
      // Rust arrives before init() finishes and the markView is rendered with
      // empty metadata.

      return created;
    } catch (err) {
      console.error('loreEditor init error:', err);
      throw err;
    }
  },

  cleanup(noteId) {
    if (activeNoteId === noteId) activeNoteId = null;
    setDirty(false);
  },

  destroy() {
    if (editor) {
      try { editor.destroy(); } catch { /* ignore */ }
      editor = null;
    }
    activeNoteId = null;
  },

  getContent() {
    if (!editor) return '';
    let md = '';
    editor.action(ctx => {
      const serializer = ctx.get(serializerCtx);
      const view = ctx.get(editorViewCtx);
      md = serializer(view.state.doc);
    });
    return md;
  },

  insertImage(name, url) {
    if (!editor) return;
    editor.action(ctx => {
      const view = ctx.get(editorViewCtx);
      const { state } = view;
      const from = state.selection.from;
      view.dispatch(state.tr.insertText(`![${name}](${url})`, from));
    });
  },

  // Insert a file link as a proper Link mark so Milkdown renders it as <a>
  // immediately (insertText would leave it as plain text until reload).
  insertFile(name, url) {
    if (!editor) return;
    editor.action(ctx => {
      const view = ctx.get(editorViewCtx);
      const { state } = view;
      const { from, to } = state.selection;
      const linkMark = state.schema.marks.link;
      let tr;
      if (linkMark) {
        const node = state.schema.text(name, [
          linkMark.create({ href: url, title: name }),
        ]);
        tr = state.tr.replaceWith(from, to, node);
      } else {
        tr = state.tr.insertText(`[${name}](${url})`, from);
      }
      view.dispatch(tr);
    });
  },

  resolveAttachments(map) {
    if (!editor) return;
    editor.action(ctx => {
      const view = ctx.get(editorViewCtx);
      const imgs = view.dom.querySelectorAll('img');
      imgs.forEach(img => {
        const src = img.getAttribute('src') || '';
        if (!src.startsWith(ATTACHMENT_URL_PREFIX)) return;
        const id = src.replace(ATTACHMENT_URL_PREFIX, '').replace(/[^0-9].*$/, '');
        if (map[id]) img.setAttribute('src', map[id]);
      });
    });
  },

  updateUrlStatuses(statuses) {
    if (!editor) return;
    editor.action(ctx => {
      const view = ctx.get(editorViewCtx);
      const links = view.dom.querySelectorAll('a[href]:not(.file-attachment-block)');
      links.forEach(a => {
        const href = a.getAttribute('href');
        const status = statuses[href];
        a.classList.remove(
          'url-archived',
          'url-queued',
          'url-external',
          'url-failed',
        );
        if (status === 'archived') a.classList.add('url-archived');
        else if (status === 'queued' || status === 'fetching') a.classList.add('url-queued');
        else if (status === 'failed') a.classList.add('url-failed');
        else a.classList.add('url-external');
      });
    });
  },

  setAttachmentMeta(map) {
    if (!map) return;
    Object.assign(attachmentMeta, map);
    refreshAllAttachmentMarkViews();
  },
};
