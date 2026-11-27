const BRIDGE_BASE = 'http://127.0.0.1:17890';
const MIN_CAPTURE_BYTES = 1024 * 1024; // 1 MiB threshold for auto-intercept
const MIN_DIRECT_MEDIA_BYTES = 512 * 1024; // Ignore tiny segment chunks (< 512 KiB)

const PLAYLIST_URL_RE = /\.(m3u8|mpd)(\?|#|$)/i;
const PLAYLIST_MIME_RE = /(mpegurl|dash\+xml)/i;
const DIRECT_MEDIA_URL_RE = /\.(mp4|webm|mkv|m4a|mp3|flac|ogg)(\?|#|$)/i;
const DIRECT_MEDIA_MIME_RE = /(video\/mp4|video\/webm|video\/x-matroska|audio\/mpeg|audio\/flac)/i;
const SEGMENT_URL_RE =
  /(\.(m4s|ts|cmfa|cmfv|fmp4)(\?|#|$)|\/(data|seg|segment|chunk|media|frag)[-_]?\d+\.)/i;

// Per-tab detected media streams: Map<tabId, Array<{ url, type, title, referrer, seenAt }>>
const tabMedia = new Map();
// Short-lived bypass set when user clicks "Browser Download" in the IDM popup
const browserBypassUrls = new Set();

async function getSettings() {
  const data = await chrome.storage.local.get({
    autoIntercept: true,
    showPopup: true,
    floatingButton: true,
  });
  return data;
}

async function isBridgeAlive() {
  try {
    const res = await fetch(`${BRIDGE_BASE}/ping`, { method: 'GET' });
    return res.ok;
  } catch {
    return false;
  }
}

async function ensureBridgeOrWake() {
  if (await isBridgeAlive()) return true;
  try {
    const tab = await chrome.tabs.create({ url: 'dandown://wake', active: false });
    if (tab?.id != null) {
      setTimeout(() => {
        chrome.tabs.remove(tab.id).catch(() => {});
      }, 800);
    }
  } catch {
    return false;
  }

  const deadline = Date.now() + 4500;
  while (Date.now() < deadline) {
    await new Promise((r) => setTimeout(r, 400));
    if (await isBridgeAlive()) return true;
  }
  return false;
}

async function getCookieHeader(url) {
  try {
    const cookies = await chrome.cookies.getAll({ url });
    return cookies.map((c) => `${c.name}=${c.value}`).join('; ');
  } catch {
    return '';
  }
}

async function sendToDanDown({
  url,
  referrer,
  title,
  filePath,
  chunkCount = 8,
  startPaused = false,
}) {
  const cookieHeader = await getCookieHeader(url);
  const res = await fetch(`${BRIDGE_BASE}/download`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      url,
      referrer: referrer || undefined,
      title: title || undefined,
      file_path: filePath || undefined,
      user_agent: navigator.userAgent,
      cookie_header: cookieHeader || undefined,
      chunk_count: chunkCount,
      start_paused: Boolean(startPaused),
    }),
  });
  const data = await res.json().catch(() => null);
  if (!res.ok) {
    throw new Error(data?.message || `danDown bridge returned HTTP ${res.status}`);
  }
  return data || { ok: true };
}

async function probeUrl(url) {
  const res = await fetch(`${BRIDGE_BASE}/probe?url=${encodeURIComponent(url)}`, {
    method: 'GET',
  });
  const data = await res.json().catch(() => null);
  if (!res.ok || !data?.ok) {
    return { ok: false };
  }
  return data;
}

async function pickFolder(currentDir) {
  const res = await fetch(
    `${BRIDGE_BASE}/pick-folder?dir=${encodeURIComponent(currentDir || '')}`,
    { method: 'POST' },
  );
  const data = await res.json().catch(() => null);
  if (!res.ok || !data?.ok) {
    throw new Error(data?.message || `Folder picker error HTTP ${res.status}`);
  }
  return data;
}

async function getDownloadStatus(downloadId) {
  const res = await fetch(`${BRIDGE_BASE}/status?id=${encodeURIComponent(downloadId)}`, {
    method: 'GET',
  });
  const data = await res.json().catch(() => null);
  if (!res.ok || !data?.ok) {
    const err = new Error(data?.message || `Status error HTTP ${res.status}`);
    if (res.status === 404) err.notFound = true;
    throw err;
  }
  return data;
}

async function controlDownload(downloadId, action) {
  let endpoint = '';
  if (action === 'pause') endpoint = `/pause?id=${encodeURIComponent(downloadId)}`;
  else if (action === 'resume') endpoint = `/resume?id=${encodeURIComponent(downloadId)}`;
  else if (action === 'cancel') endpoint = `/cancel?id=${encodeURIComponent(downloadId)}`;
  else if (action === 'open-file')
    endpoint = `/open?id=${encodeURIComponent(downloadId)}&mode=file`;
  else if (action === 'open-folder')
    endpoint = `/open?id=${encodeURIComponent(downloadId)}&mode=folder`;
  else throw new Error(`Unknown action: ${action}`);

  const res = await fetch(`${BRIDGE_BASE}${endpoint}`, { method: 'POST' });
  const data = await res.json().catch(() => null);
  if (!res.ok) {
    throw new Error(data?.message || `Action failed HTTP ${res.status}`);
  }
  return data || { ok: true };
}

async function openCapturePopup({ url, referrer, title, filename, totalBytes, kind }) {
  const captureId = `${Date.now()}_${Math.random().toString(36).slice(2, 8)}`;
  await chrome.storage.session.set({
    [`capture_${captureId}`]: {
      url,
      referrer: referrer || '',
      title: title || '',
      filename: filename || '',
      totalBytes: totalBytes || 0,
      kind: kind || 'Direct File',
    },
  });

  const width = 580;
  const height = 355;
  let left;
  let top;
  try {
    const win = await chrome.windows.getLastFocused();
    if (win && typeof win.left === 'number' && typeof win.width === 'number') {
      left = Math.max(0, Math.round(win.left + (win.width - width) / 2));
      top = Math.max(0, Math.round(win.top + (win.height - height) / 2));
    }
  } catch {
    // fallback to default OS placement
  }

  await chrome.windows.create({
    url: chrome.runtime.getURL(`capture.html?id=${encodeURIComponent(captureId)}`),
    type: 'popup',
    width,
    height,
    ...(left !== undefined ? { left, top } : {}),
    focused: true,
  });
}

function updateBadge(tabId) {
  const items = tabMedia.get(tabId) || [];
  const text = items.length > 0 ? String(Math.min(items.length, 99)) : '';
  chrome.action.setBadgeText({ tabId, text }).catch(() => {});
  chrome.action.setBadgeBackgroundColor({ tabId, color: '#27272a' }).catch(() => {});
}

async function notifyTabContent(tabId, items) {
  const { floatingButton } = await getSettings();
  if (!floatingButton) return;
  chrome.tabs.sendMessage(tabId, { type: 'MEDIA_DETECTED', items }).catch(() => {});
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
      if (type === 'HLS' || type === 'DASH') {
        list.unshift(entry);
      } else {
        list.push(entry);
      }
      if (list.length > 15) list.pop();
      tabMedia.set(tabId, list);
      updateBadge(tabId);
      notifyTabContent(tabId, list);
    })
    .catch(() => {});
}

chrome.runtime.onInstalled.addListener(() => {
  chrome.storage.local.set({ autoIntercept: true, showPopup: true, floatingButton: true });
  chrome.contextMenus.create({
    id: 'dandown-download-link',
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

    if (PLAYLIST_URL_RE.test(details.url) || PLAYLIST_MIME_RE.test(contentType)) {
      const kind = /\.mpd/i.test(details.url) || /dash\+xml/i.test(contentType) ? 'DASH' : 'HLS';
      recordMedia(details.tabId, details.url, kind, details.initiator || details.documentUrl);
      return;
    }

    if (SEGMENT_URL_RE.test(details.url)) return;

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

  if (msg?.type === 'OPEN_CAPTURE_POPUP') {
    openCapturePopup({
      url: msg.url,
      referrer: msg.referrer,
      title: msg.title,
      filename: msg.filename,
      totalBytes: msg.totalBytes,
      kind: msg.kind,
    })
      .then(() => sendResponse({ ok: true }))
      .catch((err) => sendResponse({ ok: false, error: String(err) }));
    return true;
  }

  if (msg?.type === 'CONFIRM_DOWNLOAD' || msg?.type === 'SEND_TO_DANDOWN') {
    sendToDanDown({
      url: msg.url,
      referrer: msg.referrer,
      title: msg.title,
      filePath: msg.filePath,
      chunkCount: msg.chunkCount || 8,
      startPaused: msg.startPaused || false,
    })
      .then((data) => {
        if (msg.captureId) {
          chrome.storage.session.remove(`capture_${msg.captureId}`);
        }
        sendResponse({ ok: true, downloadId: data?.download_id ?? null });
      })
      .catch((err) => sendResponse({ ok: false, error: err?.message || String(err) }));
    return true;
  }

  if (msg?.type === 'PROBE_URL') {
    probeUrl(msg.url)
      .then((data) => sendResponse({ ok: true, data }))
      .catch(() => sendResponse({ ok: false }));
    return true;
  }

  if (msg?.type === 'PICK_FOLDER') {
    pickFolder(msg.currentDir)
      .then((data) => sendResponse({ ok: true, path: data?.path || null }))
      .catch((err) => sendResponse({ ok: false, error: err?.message || String(err) }));
    return true;
  }

  if (msg?.type === 'GET_DOWNLOAD_STATUS') {
    getDownloadStatus(msg.downloadId)
      .then((data) => sendResponse({ ok: true, data }))
      .catch((err) =>
        sendResponse({
          ok: false,
          notFound: Boolean(err?.notFound),
          error: err?.message || String(err),
        }),
      );
    return true;
  }

  if (msg?.type === 'WAKE_DANDOWN') {
    ensureBridgeOrWake()
      .then((alive) => sendResponse({ ok: alive }))
      .catch(() => sendResponse({ ok: false }));
    return true;
  }

  if (msg?.type === 'CONTROL_DOWNLOAD') {
    controlDownload(msg.downloadId, msg.action)
      .then((data) => sendResponse({ ok: true, data }))
      .catch((err) => sendResponse({ ok: false, error: err?.message || String(err) }));
    return true;
  }

  if (msg?.type === 'FALLBACK_BROWSER_DOWNLOAD') {
    if (msg.captureId) {
      chrome.storage.session.remove(`capture_${msg.captureId}`);
    }
    if (msg.url) {
      browserBypassUrls.add(msg.url);
      setTimeout(() => browserBypassUrls.delete(msg.url), 15000);
      chrome.downloads.download({ url: msg.url });
    }
    sendResponse({ ok: true });
    return true;
  }

  return false;
});

chrome.contextMenus.onClicked.addListener(async (info, tab) => {
  if (info.menuItemId !== 'dandown-download-link') return;
  const targetUrl = info.linkUrl || info.srcUrl;
  if (!targetUrl || !/^https?:\/\//i.test(targetUrl)) return;

  await ensureBridgeOrWake();
  const { showPopup } = await getSettings();
  if (showPopup) {
    await openCapturePopup({
      url: targetUrl,
      referrer: tab?.url || info.pageUrl,
      title: tab?.title,
      kind: 'Link',
    });
  } else {
    await sendToDanDown({
      url: targetUrl,
      referrer: tab?.url || info.pageUrl,
      title: tab?.title,
    }).catch(() => {});
  }
});

chrome.downloads.onCreated.addListener(async (item) => {
  const targetUrl = item.finalUrl || item.url;
  if (!targetUrl || !/^https?:\/\//i.test(targetUrl)) return;

  if (browserBypassUrls.has(targetUrl) || browserBypassUrls.has(item.url)) {
    browserBypassUrls.delete(targetUrl);
    browserBypassUrls.delete(item.url);
    return;
  }

  if (item.totalBytes > 0 && item.totalBytes < MIN_CAPTURE_BYTES) return;

  const { autoIntercept, showPopup } = await getSettings();
  if (!autoIntercept) return;

  if (!(await ensureBridgeOrWake())) return;

  chrome.downloads.cancel(item.id, () => {
    chrome.downloads.erase({ id: item.id });
  });

  const inferredName =
    (item.filename || '').split(/[/\\]/).pop() ||
    decodeURIComponent(targetUrl.split('/').pop()?.split('?')[0] || '');

  if (showPopup) {
    await openCapturePopup({
      url: targetUrl,
      referrer: item.referrer,
      filename: inferredName,
      totalBytes: item.totalBytes,
      kind: item.mime || 'Direct File',
    });
  } else {
    await sendToDanDown({
      url: targetUrl,
      referrer: item.referrer,
      title: inferredName,
    }).catch(() => {});
  }
});
