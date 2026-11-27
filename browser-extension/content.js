(() => {
  if (window !== window.top) return;

  const HOST_ID = 'dandown-floating-bar-host';
  let currentItems = [];
  let dismissed = false;
  let expanded = false;
  let shadowRoot = null;

  function ensureHost() {
    let host = document.getElementById(HOST_ID);
    if (host) return host;

    host = document.createElement('div');
    host.id = HOST_ID;
    host.style.position = 'fixed';
    host.style.top = '16px';
    host.style.right = '16px';
    host.style.zIndex = '2147483647';
    shadowRoot = host.attachShadow({ mode: 'open' });
    document.documentElement.appendChild(host);
    return host;
  }

  function render() {
    const existing = document.getElementById(HOST_ID);
    if (dismissed || currentItems.length === 0) {
      if (existing) existing.remove();
      return;
    }

    ensureHost();
    const primary = currentItems[0];

    shadowRoot.innerHTML = `
      <style>
        .wrap {
          font-family: system-ui, -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
          color: #fafafa;
        }
        .pill {
          display: inline-flex;
          align-items: center;
          gap: 8px;
          background: #141414;
          border: 1px solid #2a2a2a;
          border-radius: 999px;
          padding: 6px 10px 6px 10px;
          box-shadow: 0 10px 28px rgba(0, 0, 0, 0.5);
          font-size: 12px;
          font-weight: 600;
        }
        .main-btn {
          all: unset;
          display: inline-flex;
          align-items: center;
          gap: 7px;
          cursor: pointer;
          color: #fafafa;
        }
        .icon {
          width: 20px;
          height: 20px;
          border-radius: 999px;
          background: #fafafa;
          display: grid;
          place-items: center;
          font-size: 12px;
          font-weight: 800;
          color: #171717;
        }
        .count {
          background: #262626;
          border: 1px solid #323232;
          color: #fafafa;
          font-size: 10px;
          padding: 1px 6px;
          border-radius: 999px;
        }
        .close-btn {
          all: unset;
          cursor: pointer;
          color: #a1a1aa;
          font-size: 14px;
          line-height: 1;
          padding: 0 3px;
        }
        .close-btn:hover { color: #fafafa; }
        .menu {
          margin-top: 6px;
          width: 290px;
          max-height: 220px;
          overflow-y: auto;
          background: #141414;
          border: 1px solid #2a2a2a;
          border-radius: 10px;
          padding: 6px;
          box-shadow: 0 12px 30px rgba(0, 0, 0, 0.55);
          display: flex;
          flex-direction: column;
          gap: 5px;
        }
        .item {
          all: unset;
          display: flex;
          align-items: center;
          gap: 8px;
          padding: 6px 8px;
          border-radius: 7px;
          cursor: pointer;
          background: #1d1d1d;
          border: 1px solid #2a2a2a;
          font-size: 12px;
        }
        .item:hover { background: #262626; }
        .tag {
          font-size: 10px;
          font-weight: 700;
          padding: 1px 5px;
          border-radius: 4px;
          background: #fafafa;
          color: #171717;
          flex-shrink: 0;
        }
        .name {
          overflow: hidden;
          text-overflow: ellipsis;
          white-space: nowrap;
          flex: 1;
        }
      </style>
      <div class="wrap">
        <div class="pill">
          <button class="main-btn" id="dd-primary">
            <span class="icon">↓</span>
            <span>Download with danDown</span>
            ${currentItems.length > 1 ? `<span class="count">${currentItems.length}</span>` : ''}
          </button>
          <button class="close-btn" id="dd-close" title="Dismiss">×</button>
        </div>
        ${expanded && currentItems.length > 1 ? `<div class="menu" id="dd-menu"></div>` : ''}
      </div>
    `;

    shadowRoot.getElementById('dd-primary').addEventListener('click', () => {
      if (currentItems.length === 1) {
        chrome.runtime.sendMessage({
          type: 'OPEN_CAPTURE_POPUP',
          url: primary.url,
          title: primary.title || document.title,
          referrer: primary.referrer || window.location.href,
          kind: primary.type,
        });
      } else {
        expanded = !expanded;
        render();
      }
    });

    shadowRoot.getElementById('dd-close').addEventListener('click', () => {
      dismissed = true;
      render();
    });

    const menu = shadowRoot.getElementById('dd-menu');
    if (menu) {
      for (const item of currentItems) {
        const btn = document.createElement('button');
        btn.className = 'item';
        const tag = document.createElement('span');
        tag.className = 'tag';
        tag.textContent = item.type;
        const name = document.createElement('span');
        name.className = 'name';
        name.textContent = item.title || item.url.split('/').pop()?.split('?')[0] || item.url;
        btn.append(tag, name);
        btn.addEventListener('click', () => {
          expanded = false;
          render();
          chrome.runtime.sendMessage({
            type: 'OPEN_CAPTURE_POPUP',
            url: item.url,
            title: item.title || document.title,
            referrer: item.referrer || window.location.href,
            kind: item.type,
          });
        });
        menu.appendChild(btn);
      }
    }
  }

  chrome.runtime.onMessage.addListener((msg) => {
    if (msg?.type === 'MEDIA_DETECTED') {
      currentItems = Array.isArray(msg.items) ? msg.items : [];
      if (currentItems.length > 0) {
        dismissed = false;
      }
      render();
    }
  });
})();
