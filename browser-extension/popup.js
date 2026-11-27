const statusEl = document.getElementById('status');
const toggleIntercept = document.getElementById('toggle-intercept');
const togglePopup = document.getElementById('toggle-popup');
const toggleFloating = document.getElementById('toggle-floating');
const mediaListEl = document.getElementById('media-list');

chrome.storage.local.get(
  { autoIntercept: true, showPopup: true, floatingButton: true },
  (prefs) => {
    toggleIntercept.checked = Boolean(prefs.autoIntercept);
    togglePopup.checked = Boolean(prefs.showPopup);
    toggleFloating.checked = Boolean(prefs.floatingButton);
  },
);

toggleIntercept.addEventListener('change', () => {
  chrome.storage.local.set({ autoIntercept: toggleIntercept.checked });
});
togglePopup.addEventListener('change', () => {
  chrome.storage.local.set({ showPopup: togglePopup.checked });
});
toggleFloating.addEventListener('change', () => {
  chrome.storage.local.set({ floatingButton: toggleFloating.checked });
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
      label.textContent = item.title || item.url.split('/').pop()?.split('?')[0] || item.url;

      const btn = document.createElement('button');
      btn.textContent = 'Download';
      btn.addEventListener('click', () => {
        if (togglePopup.checked) {
          chrome.runtime.sendMessage({
            type: 'OPEN_CAPTURE_POPUP',
            url: item.url,
            title: item.title || activeTab.title,
            referrer: item.referrer || activeTab.url,
            kind: item.type,
          });
          window.close();
        } else {
          btn.disabled = true;
          btn.textContent = 'Sent';
          chrome.runtime.sendMessage({
            type: 'SEND_TO_DANDOWN',
            url: item.url,
            title: item.title || activeTab.title,
            referrer: item.referrer || activeTab.url,
          });
        }
      });

      row.append(kind, label, btn);
      mediaListEl.appendChild(row);
    }
  });
});
