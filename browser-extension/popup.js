const statusEl = document.getElementById('status');
const toggleEl = document.getElementById('toggle');
const mediaListEl = document.getElementById('media-list');

chrome.storage.local.get('autoIntercept', ({ autoIntercept = true }) => {
  toggleEl.checked = Boolean(autoIntercept);
});

toggleEl.addEventListener('change', () => {
  chrome.storage.local.set({ autoIntercept: toggleEl.checked });
});

fetch('http://127.0.0.1:17890/ping')
  .then((res) => {
    statusEl.textContent = res.ok ? 'Connected' : 'Offline';
    statusEl.className = `badge ${res.ok ? 'online' : 'offline'}`;
  })
  .catch(() => {
    statusEl.textContent = 'Offline';
    statusEl.className = 'badge offline';
  });

chrome.tabs.query({ active: true, currentWindow: true }, (tabs) => {
  const activeTab = tabs[0];
  if (!activeTab?.id) return;

  chrome.runtime.sendMessage({ type: 'GET_TAB_MEDIA', tabId: activeTab.id }, (resp) => {
    const items = resp?.items || [];
    if (items.length === 0) return;

    mediaListEl.innerHTML = '';
    for (const item of items) {
      const row = document.createElement('div');
      row.className = 'media-item';

      const kind = document.createElement('span');
      kind.className = 'media-kind';
      kind.textContent = item.type;

      const label = document.createElement('span');
      label.className = 'media-url';
      label.title = item.url;
      label.textContent =
        item.title || item.url.split('/').pop()?.split('?')[0] || item.url;

      const btn = document.createElement('button');
      btn.textContent = 'Download';
      btn.addEventListener('click', () => {
        btn.disabled = true;
        btn.textContent = 'Sent';
        chrome.runtime.sendMessage({
          type: 'SEND_TO_danDown',
          url: item.url,
          title: item.title || activeTab.title,
          referrer: item.referrer || activeTab.url,
        });
      });

      row.append(kind, label, btn);
      mediaListEl.appendChild(row);
    }
  });
});
