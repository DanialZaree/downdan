const BRIDGE_BASE = 'http://127.0.0.1:17890';
const MIN_CAPTURE_BYTES = 1024 * 1024; // 1 MiB threshold for auto-intercept
const MIN_DIRECT_MEDIA_BYTES = 512 * 1024; // Ignore tiny segment chunks (< 512 KiB)

const PLAYLIST_URL_RE = /\.(m3u8|mpd)(\?|#|$)/i;
const PLAYLIST_MIME_RE = /(mpegurl|dash\+xml)/i;
const DIRECT_MEDIA_URL_RE = /\.(mp4|webm|mkv|m4a|mp3|flac|ogg)(\?|#|$)/i;
const DIRECT_MEDIA_MIME_RE = /(video\/mp4|video\/webm|video\/x-matroska|audio\/mpeg|audio\/flac)/i;
// Individual HLS/DASH fragments that are unplayable on their own
const SEGMENT_URL_RE =
  /(\.(m4s|ts|cmfa|cmfv|fmp4)(\?|#|$)|\/(data|seg|segment|chunk|media|frag)[-_]?\d+\.)/i;

// Per-tab detected media streams: Map<tabId, Array<{ url, type, title, referrer, seenAt }>>
const tabMedia = new Map();

async function isBridgeAlive() {
  try {
    const res = await fetch(`${BRIDGE_BASE}/ping`, { method: 'GET' });
    return res.ok;
  } catch {
    return false;
  }
}

async function getCookieHeader(url) {
  try {
    const cookies = await chrome.cookies.getAll({ url });
    return cookies.map((c) => `${c.name}=${c.value}`).join('; ');
  } catch {
    return '';
  }
}

async function sendTodanDown({ url, referrer, title }) {
  const cookieHeader = await getCookieHeader(url);
  const res = await fetch(`${BRIDGE_BASE}/download`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      url,
      referrer: referrer || undefined,
      title: title || undefined,
      user_agent: navigator.userAgent,
      cookie_header: cookieHeader || undefined,
      chunk_count: 8,
    }),
  });
  return res.ok;
}

function updateBadge(tabId) {
  const items = tabMedia.get(tabId) || [];
  const text = items.length > 0 ? String(Math.min(items.length, 99)) : '';
  chrome.action.setBadgeText({ tabId, text }).catch(() => {});
  chrome.action.setBadgeBackgroundColor({ tabId, color: '#2563eb' }).catch(() => {});
}

function recordMedia(tabId, url, type, referrer) {
  if (tabId < 0 || !/^https?:\/\//i.test(url)) return;
  const list = tabMedia.get(tabId) || [];
  if (list.some((item) => item.url === url)) return;

  chrome.tabs
    .get(tabId)
    .then((tab) => {
      const title = (tab?.title || '').replace(/\s*[|–—-]\s*SoundCloud.*$/i, '').trim();
      const entry = { url, type, title, referrer: tab?.url || referrer, seenAt: Date.now() };
      // Keep full playlists (HLS/DASH) pinned at the top ahead of direct media.
      if (type === 'HLS' || type === 'DASH') {
        list.unshift(entry);
      } else {
        list.push(entry);
      }
      if (list.length > 15) list.pop();
      tabMedia.set(tabId, list);
      updateBadge(tabId);
    })
    .catch(() => {});
}

chrome.runtime.onInstalled.addListener(() => {
  chrome.storage.local.set({ autoIntercept: true });
  chrome.contextMenus.create({
    id: 'danDown-download-link',
    title: 'Download with danDown',
    contexts: ['link', 'video', 'audio'],
  });
});

chrome.tabs.onRemoved.addListener((tabId) => {
  tabMedia.delete(tabId);
});

chrome.tabs.onUpdated.addListener((tabId, changeInfo) => {
  if (changeInfo.status === 'loading') {
    tabMedia.delete(tabId);
    updateBadge(tabId);
  }
});

chrome.webRequest.onHeadersReceived.addListener(
  (details) => {
    if (details.tabId < 0 || details.method !== 'GET') return;
    if (details.statusCode && details.statusCode >= 400) return;

    const getHeader = (name) =>
      details.responseHeaders?.find((h) => h.name.toLowerCase() === name)?.value || '';

    const contentType = getHeader('content-type');
    const contentLength = Number.parseInt(getHeader('content-length') || '0', 10) || 0;

    // 1. Always capture full HLS (.m3u8) / DASH (.mpd) playlists.
    if (PLAYLIST_URL_RE.test(details.url) || PLAYLIST_MIME_RE.test(contentType)) {
      const kind =
        /\.mpd/i.test(details.url) || /dash\+xml/i.test(contentType) ? 'DASH' : 'HLS';
      recordMedia(details.tabId, details.url, kind, details.initiator || details.documentUrl);
      return;
    }

    // 2. Skip individual fragmented segments (.m4s, .ts, data000.m4s, seg-1.m4s, etc.).
    if (SEGMENT_URL_RE.test(details.url)) return;

    // 3. Capture full direct media files (skip tiny preview/chunk responses).
    if (DIRECT_MEDIA_URL_RE.test(details.url) || DIRECT_MEDIA_MIME_RE.test(contentType)) {
      if (contentLength > 0 && contentLength < MIN_DIRECT_MEDIA_BYTES) return;
      recordMedia(details.tabId, details.url, 'MEDIA', details.initiator || details.documentUrl);
    }
  },
  { urls: ['<all_urls>'] },
  ['responseHeaders'],
);

chrome.runtime.onMessage.addListener((msg, _sender, sendResponse) => {
  if (msg?.type === 'GET_TAB_MEDIA') {
    sendResponse({ items: tabMedia.get(msg.tabId) || [] });
    return true;
  }
  if (msg?.type === 'SEND_TO_danDown') {
    sendTodanDown({ url: msg.url, referrer: msg.referrer, title: msg.title })
      .then((ok) => sendResponse({ ok }))
      .catch(() => sendResponse({ ok: false }));
    return true;
  }
  return false;
});

chrome.contextMenus.onClicked.addListener(async (info, tab) => {
  if (info.menuItemId !== 'danDown-download-link') return;
  const targetUrl = info.linkUrl || info.srcUrl;
  if (!targetUrl || !/^https?:\/\//i.test(targetUrl)) return;

  await sendTodanDown({
    url: targetUrl,
    referrer: tab?.url || info.pageUrl,
    title: tab?.title,
  });
});

chrome.downloads.onCreated.addListener(async (item) => {
  if (!item.url || !/^https?:\/\//i.test(item.url)) return;
  if (item.totalBytes > 0 && item.totalBytes < MIN_CAPTURE_BYTES) return;

  const { autoIntercept = true } = await chrome.storage.local.get('autoIntercept');
  if (!autoIntercept) return;

  if (!(await isBridgeAlive())) return;

  const sent = await sendTodanDown({
    url: item.finalUrl || item.url,
    referrer: item.referrer,
  });

  if (sent) {
    chrome.downloads.cancel(item.id, () => {
      chrome.downloads.erase({ id: item.id });
    });
  }
});
