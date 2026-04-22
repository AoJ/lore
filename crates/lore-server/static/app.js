const API = '';
const main = document.getElementById('main-content');

// Router
function route() {
    const hash = location.hash.slice(1) || '/';
    const parts = hash.split('/').filter(Boolean);

    document.querySelectorAll('.nav-link').forEach(a => {
        a.classList.toggle('active', a.getAttribute('href') === '#/' + (parts[0] || ''));
    });

    if (parts[0] === 'page' && parts[1]) {
        renderPageDetail(parseInt(parts[1]));
    } else if (parts[0] === 'rules') {
        renderRules();
    } else {
        renderPageList();
    }
}

window.addEventListener('hashchange', route);

// Search
const searchInput = document.getElementById('search-input');
const searchResults = document.getElementById('search-results');
let searchTimeout;

searchInput.addEventListener('input', () => {
    clearTimeout(searchTimeout);
    const q = searchInput.value.trim();
    if (q.length < 2) {
        searchResults.innerHTML = '';
        return;
    }
    searchTimeout = setTimeout(async () => {
        const res = await fetch(`${API}/api/search?q=${encodeURIComponent(q)}&limit=20`);
        const data = await res.json();
        searchResults.innerHTML = data.map(r =>
            `<li><a href="#/page/${r.id}">${esc(r.title || '(no title)')}</a> <small>(${esc(r.domain)})</small></li>`
        ).join('');
    }, 200);
});

// Add URL
document.getElementById('add-form').addEventListener('submit', async (e) => {
    e.preventDefault();
    const input = document.getElementById('add-url');
    const url = input.value.trim();
    if (!url) return;

    const status = document.getElementById('add-status');
    try {
        const res = await fetch(`${API}/api/pages`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ url })
        });
        const data = await res.json();
        status.innerHTML = `<small class="status-msg">[${data.category}] added</small>`;
        input.value = '';
        if (location.hash === '#/' || location.hash === '') renderPageList();
    } catch (err) {
        status.innerHTML = `<small class="status-msg">Error: ${err.message}</small>`;
    }
});

// Page list
async function renderPageList() {
    const res = await fetch(`${API}/api/pages?limit=100`);
    const pages = await res.json();

    main.innerHTML = `
        <section class="page-list">
            <h2>Pages</h2>
            <table role="grid">
                <thead>
                    <tr><th>Title</th><th>Domain</th><th>Category</th><th>Status</th><th>Added</th></tr>
                </thead>
                <tbody>
                    ${pages.map(p => `
                        <tr>
                            <td><a href="#/page/${p.id}">${esc(p.title || '(no title)')}</a></td>
                            <td>${esc(p.domain)}</td>
                            <td>${esc(p.category)}</td>
                            <td>${esc(p.status)}</td>
                            <td class="date">${esc(p.created_at.slice(0, 10))}</td>
                        </tr>
                    `).join('')}
                </tbody>
            </table>
        </section>
    `;
}

// Page detail
async function renderPageDetail(id) {
    const res = await fetch(`${API}/api/pages/${id}`);
    if (!res.ok) {
        main.innerHTML = '<p>Page not found</p>';
        return;
    }
    const p = await res.json();

    let contentHtml = '';
    if (p.has_snapshot) {
        contentHtml = `
            <div class="page-actions">
                <a href="${esc(p.url)}" target="_blank" role="button" class="outline">Open in browser</a>
            </div>
        `;
        if (p.plain_text_preview) {
            contentHtml += `
                <details open>
                    <summary>Content preview</summary>
                    <pre class="content-preview">${esc(p.plain_text_preview)}</pre>
                </details>
            `;
        }
    }

    main.innerHTML = `
        <section class="page-detail">
            <h2>${esc(p.title || '(no title)')}</h2>
            <dl>
                <dt>URL</dt><dd><a href="${esc(p.url)}" target="_blank">${esc(p.url)}</a></dd>
                <dt>Domain</dt><dd>${esc(p.domain)}</dd>
                <dt>Category</dt><dd>${esc(p.category)}</dd>
                <dt>Status</dt><dd>${esc(p.status)}</dd>
                <dt>Added</dt><dd>${esc(p.created_at.slice(0, 10))}</dd>
                ${p.content_size ? `<dt>Content size</dt><dd>${formatSize(p.content_size)}</dd>` : ''}
            </dl>
            ${contentHtml}
        </section>
    `;
}

// Rules
async function renderRules() {
    const res = await fetch(`${API}/api/rules`);
    const rules = await res.json();

    main.innerHTML = `
        <section class="rules">
            <h2>Classification Rules</h2>
            <table role="grid">
                <thead>
                    <tr><th>Pattern</th><th>Match type</th><th>Category</th><th>Priority</th><th>Note</th></tr>
                </thead>
                <tbody>
                    ${rules.map(r => `
                        <tr>
                            <td><code>${esc(r.pattern)}</code></td>
                            <td>${esc(r.match_type)}</td>
                            <td>${esc(r.category)}</td>
                            <td>${r.priority}</td>
                            <td>${esc(r.note || '')}</td>
                        </tr>
                    `).join('')}
                </tbody>
            </table>
        </section>
    `;
}

function esc(s) {
    const d = document.createElement('div');
    d.textContent = s;
    return d.innerHTML;
}

function formatSize(bytes) {
    if (bytes > 1_000_000) return (bytes / 1_000_000).toFixed(1) + ' MB';
    if (bytes > 1_000) return (bytes / 1_000).toFixed(1) + ' KB';
    return bytes + ' B';
}

route();
