const params = new URLSearchParams(window.location.search);
const captureId = params.get('id');

// Views
const setupView = document.getElementById('setup-view');
const progressView = document.getElementById('progress-view');

// Popup 1 elements
const urlInputEl = document.getElementById('url-input');
const btnCopyUrl = document.getElementById('btn-copy-url');
const p1CopyTagEl = document.getElementById('p1-copy-tag');
const fileNameEl = document.getElementById('file-name');
const saveDirEl = document.getElementById('save-dir');
const btnBrowse = document.getElementById('btn-browse');

// Custom dropdowns
const catTrigger = document.getElementById('cat-trigger');
const catLabelEl = document.getElementById('cat-label');
const catMenuEl = document.getElementById('cat-menu');
const btnAddCategory = document.getElementById('btn-add-category');
const newCatRowEl = document.getElementById('new-cat-row');
const newCatInputEl = document.getElementById('new-cat-input');
const btnSaveCat = document.getElementById('btn-save-cat');
const btnCancelCat = document.getElementById('btn-cancel-cat');

const connTrigger = document.getElementById('conn-trigger');
const connLabelEl = document.getElementById('conn-label');
const connMenuEl = document.getElementById('conn-menu');

const mimeTypeEl = document.getElementById('mime-type');
const rememberPathEl = document.getElementById('remember-path');
const rememberPathLabelEl = document.getElementById('remember-path-label');
const btnToggleNote = document.getElementById('btn-toggle-note');
const noteRowEl = document.getElementById('note-row');
const descriptionEl = document.getElementById('description');
const fileIconEl = document.getElementById('file-icon');
const extBadgeEl = document.getElementById('ext-badge');
const sizeReadoutEl = document.getElementById('size-readout');
const p1ErrorEl = document.getElementById('p1-error');
const btnBrowser = document.getElementById('btn-browser');
const btnLater = document.getElementById('btn-later');
const btnCancel = document.getElementById('btn-cancel');
const btnStart = document.getElementById('btn-start');

// Popup 2 elements
const tabStatusBtn = document.getElementById('tab-status');
const tabOptionsBtn = document.getElementById('tab-options');
const panelStatus = document.getElementById('panel-status');
const panelOptions = document.getElementById('panel-options');

const p2IconEl = document.getElementById('p2-icon');
const p2FilenameEl = document.getElementById('p2-filename');
const p2UrlEl = document.getElementById('p2-url');
const copyFeedbackEl = document.getElementById('copy-feedback');
const p2StatusBadgeEl = document.getElementById('p2-status-badge');
const p2StatusTextEl = document.getElementById('p2-status-text');

const uibrikSubEl = document.getElementById('uibrik-sub');
const p2BarFillEl = document.getElementById('p2-bar-fill');
const p2BarPctEl = document.getElementById('p2-bar-pct');

const statSpeedEl = document.getElementById('stat-speed');
const statEtaEl = document.getElementById('stat-eta');
const statResumeDotEl = document.getElementById('stat-resume-dot');
const statResumeEl = document.getElementById('stat-resume');
const statConnsEl = document.getElementById('stat-conns');

const chartCurrentSpeedEl = document.getElementById('chart-current-speed');
const chartPeakEl = document.getElementById('chart-peak');
const netAreaEl = document.getElementById('net-area');
const netLineEl = document.getElementById('net-line');

const BRIDGE_BASE = 'http://127.0.0.1:17890';

const segmentsSummaryEl = document.getElementById('segments-summary');
const segmentsBarEl = document.getElementById('segments-bar');
const detailsPanelEl = document.getElementById('details-panel');
const connTbodyEl = document.getElementById('conn-tbody');
const p2ErrorEl = document.getElementById('p2-error');

const btnPrimaryAction = document.getElementById('btn-primary-action');
const btnOpenFolder = document.getElementById('btn-open-folder');
const btnToggleDetails = document.getElementById('btn-toggle-details');
const detailsBtnTextEl = document.getElementById('details-btn-text');
const detailsChevronEl = document.getElementById('details-chevron');
const btnProgCancel = document.getElementById('btn-prog-cancel');

// Completion option toggles
const optOpenFile = document.getElementById('opt-open-file');
const optOpenFolder = document.getElementById('opt-open-folder');
const optCloseDialog = document.getElementById('opt-close-dialog');
const optShutdown = document.getElementById('opt-shutdown');

let captureData = null;
let activeDownloadId = null;
let pollTimer = null;
let lastStatus = 'downloading';
let detailsOpen = false;
let completionHandled = false;

// State for custom dropdowns
const DEFAULT_CATEGORIES = [
  'General',
  'Compressed',
  'Video',
  'Music',
  'Programs',
  'Documents',
  'Images',
];
const CONNECTION_OPTIONS = [
  { value: 1, label: '1 connection' },
  { value: 4, label: '4 connections' },
  { value: 8, label: '8 connections' },
  { value: 12, label: '12 connections' },
  { value: 16, label: '16 connections' },
];
let selectedCategory = 'General';
let selectedChunks = 8;
let categoryPaths = {};
let customCategories = [];

// =========================================================================
// Live Cache & Chart State (Preserves all data + curve on Pause)
// =========================================================================
const POLL_INTERVAL_MS = 500;
const WINDOW_SAMPLES = 120; // 60 seconds @ 500ms
const WARMUP_SAMPLES = 4; // Ignore first 2s socket burst
const rawNet = [];
let totalSampleCount = 0;
let allTimePeakBps = 0;

let displayedYMax = 1024 * 1024;
let targetYMax = 1024 * 1024;
let yAnimFrame = null;

const cachedState = {
  downloaded: 0,
  total: 0,
  speedBps: 0,
  etaSecs: 0,
  supportsRange: true,
  chunkCount: 8,
  chunks: [],
};

const ICONS = {
  archive: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round"><path d="M10 12v-1"/><path d="M10 18v-2"/><path d="M10 7V6"/><path d="M14 2v4a2 2 0 0 0 2 2h4"/><path d="M15.5 22H18a2 2 0 0 0 2-2V7l-5-5H6a2 2 0 0 0-2 2v16a2 2 0 0 0 .274 1.01"/><circle cx="10" cy="20" r="2"/></svg>`,
  video: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round"><path d="M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z"/><path d="M14 2v4a2 2 0 0 0 2 2h4"/><path d="m10 11 5 3-5 3v-6Z"/></svg>`,
  audio: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round"><path d="M17.5 22h.5a2 2 0 0 0 2-2V7l-5-5H6a2 2 0 0 0-2 2v3"/><path d="M14 2v4a2 2 0 0 0 2 2h4"/><path d="M2 19a2 2 0 1 1 4 0v1a2 2 0 1 1-4 0v-4a6 6 0 0 1 12 0v4a2 2 0 1 1-4 0v-1a2 2 0 1 1 4 0"/></svg>`,
  image: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round"><path d="M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z"/><path d="M14 2v4a2 2 0 0 0 2 2h4"/><circle cx="10" cy="12" r="2"/><path d="m20 17-1.296-1.296a2.41 2.41 0 0 0-3.408 0L9 22"/></svg>`,
  document: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round"><path d="M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z"/><path d="M14 2v4a2 2 0 0 0 2 2h4"/><path d="M10 9H8"/><path d="M16 13H8"/><path d="M16 17H8"/></svg>`,
  program: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round"><path d="M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z"/><path d="M14 2v4a2 2 0 0 0 2 2h4"/><path d="m10 13-2 2 2 2"/><path d="m14 17 2-2-2-2"/></svg>`,
  file: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round"><path d="M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z"/><path d="M14 2v4a2 2 0 0 0 2 2h4"/></svg>`,
};

const CHECK_SVG = `<svg class="cselect-check" width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><polyline points="20 6 9 17 4 12"/></svg>`;

const EXT_TO_MIME = {
  rar: 'application/x-rar-compressed',
  zip: 'application/zip',
  '7z': 'application/x-7z-compressed',
  tar: 'application/x-tar',
  gz: 'application/gzip',
  mp4: 'video/mp4',
  mkv: 'video/x-matroska',
  webm: 'video/webm',
  mov: 'video/quicktime',
  avi: 'video/x-msvideo',
  mp3: 'audio/mpeg',
  flac: 'audio/flac',
  wav: 'audio/wav',
  ogg: 'audio/ogg',
  m4a: 'audio/mp4',
  exe: 'application/vnd.microsoft.portable-executable',
  msi: 'application/x-msi',
  pdf: 'application/pdf',
  png: 'image/png',
  jpg: 'image/jpeg',
  jpeg: 'image/jpeg',
  webp: 'image/webp',
  iso: 'application/x-iso9660-image',
};

// =========================================================================
// Exact Content-Aware Window Resizer (zero scrollbars in Popup 1 & Popup 2)
// =========================================================================

function fitWindowToContent(targetWidth, recenter = false) {
  requestAnimationFrame(() => {
    const activeShell = progressView.classList.contains('hidden') ? setupView : progressView;
    const contentH = Math.ceil(activeShell.getBoundingClientRect().height);
    if (contentH <= 0) return;

    const frameH = Math.max(28, (window.outerHeight || 0) - (window.innerHeight || 0));
    const desiredOuterH = contentH + frameH;

    chrome.windows?.getCurrent?.((win) => {
      if (!win?.id) return;
      const patch = { width: targetWidth, height: desiredOuterH };
      if (recenter && typeof win.left === 'number' && typeof win.width === 'number') {
        patch.left = Math.max(0, Math.round(win.left + (win.width - targetWidth) / 2));
        patch.top = Math.max(0, Math.round(win.top + (win.height - desiredOuterH) / 2));
      }
      chrome.windows.update(win.id, patch, () => {
        requestAnimationFrame(() => {
          const diff = Math.ceil(activeShell.getBoundingClientRect().height) - window.innerHeight;
          if (Math.abs(diff) > 1) {
            chrome.windows.update(win.id, { height: window.outerHeight + diff }).catch(() => {});
          }
        });
      });
    });
  });
}

function formatBytes(bytes) {
  if (!bytes || bytes <= 0) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  let idx = 0;
  let val = bytes;
  while (val >= 1024 && idx < units.length - 1) {
    val /= 1024;
    idx += 1;
  }
  return `${val.toFixed(idx === 0 ? 0 : 1)} ${units[idx]}`;
}

function formatSpeed(bps) {
  if (!bps || bps <= 0) return '0 B/s';
  return `${formatBytes(bps)}/s`;
}

function formatEta(secs, isFinishingSubSecond = false) {
  if (isFinishingSubSecond) return '< 1s';
  if (!secs || secs <= 0 || !Number.isFinite(secs)) return '--';
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  const s = Math.floor(secs % 60);
  const pad = (n) => String(n).padStart(2, '0');
  return h > 0 ? `${h}:${pad(m)}:${pad(s)}` : `${pad(m)}:${pad(s)}`;
}

function getExtension(name, url) {
  const clean = (name || '').trim();
  const m1 = clean.match(/\.([a-z0-9]{1,8})$/i);
  if (m1) return m1[1].toLowerCase();
  try {
    const m2 = new URL(url || '').pathname.match(/\.([a-z0-9]{1,8})$/i);
    if (m2) return m2[1].toLowerCase();
  } catch {
    // ignore
  }
  return '';
}

function classifyExtension(ext, mime) {
  const k = (mime || '').toLowerCase();
  if (
    ['zip', 'rar', '7z', 'tar', 'gz', 'bz2', 'xz', 'iso'].includes(ext) ||
    k.includes('rar') ||
    k.includes('zip')
  ) {
    return { category: 'Compressed', icon: 'archive' };
  }
  if (
    ['mp4', 'mkv', 'webm', 'mov', 'avi', 'flv', 'wmv', 'm3u8', 'mpd'].includes(ext) ||
    k.startsWith('video/')
  ) {
    return { category: 'Video', icon: 'video' };
  }
  if (['mp3', 'wav', 'flac', 'aac', 'ogg', 'm4a', 'opus'].includes(ext) || k.startsWith('audio/')) {
    return { category: 'Music', icon: 'audio' };
  }
  if (['exe', 'msi', 'dmg', 'pkg', 'deb', 'rpm', 'appimage', 'apk'].includes(ext)) {
    return { category: 'Programs', icon: 'program' };
  }
  if (
    ['pdf', 'doc', 'docx', 'xls', 'xlsx', 'ppt', 'pptx', 'txt', 'csv', 'epub', 'rtf'].includes(ext)
  ) {
    return { category: 'Documents', icon: 'document' };
  }
  if (
    ['png', 'jpg', 'jpeg', 'gif', 'webp', 'svg', 'bmp', 'ico'].includes(ext) ||
    k.startsWith('image/')
  ) {
    return { category: 'Images', icon: 'image' };
  }
  return { category: 'General', icon: 'file' };
}

function resolveMimeDisplay(rawMime, ext) {
  const m = (rawMime || '').trim().toLowerCase();
  if (m && m !== 'text/plain' && m !== 'direct file' && m !== 'link' && m.includes('/')) {
    return m;
  }
  if (ext && EXT_TO_MIME[ext]) return EXT_TO_MIME[ext];
  return m || 'application/octet-stream';
}

function renderSizeReadout(bytes, isLoading) {
  if (isLoading) {
    sizeReadoutEl.className = 'size-readout num';
    sizeReadoutEl.innerHTML = '<span class="skeleton-pill" aria-label="Fetching file size"></span>';
    return;
  }
  if (bytes && bytes > 0) {
    sizeReadoutEl.className = 'size-readout num';
    sizeReadoutEl.textContent = formatBytes(bytes);
  } else {
    sizeReadoutEl.className = 'size-readout muted';
    sizeReadoutEl.textContent = 'Unknown size';
  }
}

function showP1Error(msg) {
  if (msg) {
    p1ErrorEl.textContent = msg;
    p1ErrorEl.classList.remove('hidden');
  } else {
    p1ErrorEl.textContent = '';
    p1ErrorEl.classList.add('hidden');
  }
  fitWindowToContent(580, false);
}

// =========================================================================
// Custom Dark Dropdown Helpers
// =========================================================================

function closeAllMenus() {
  catMenuEl.classList.add('hidden');
  catTrigger.setAttribute('aria-expanded', 'false');
  connMenuEl.classList.add('hidden');
  connTrigger.setAttribute('aria-expanded', 'false');
}

function renderCategoryMenu() {
  const all = [...DEFAULT_CATEGORIES];
  for (const c of customCategories) {
    if (!all.some((x) => x.toLowerCase() === c.toLowerCase())) all.push(c);
  }
  catMenuEl.innerHTML = '';
  all.forEach((cat) => {
    const btn = document.createElement('button');
    btn.type = 'button';
    btn.role = 'option';
    const isSel = cat === selectedCategory;
    btn.className = isSel ? 'cselect-item selected' : 'cselect-item';
    btn.setAttribute('aria-selected', String(isSel));
    btn.innerHTML = `<span>${cat}</span>${isSel ? CHECK_SVG : ''}`;
    btn.addEventListener('click', (e) => {
      e.stopPropagation();
      setSelectedCategory(cat);
      closeAllMenus();
    });
    catMenuEl.appendChild(btn);
  });
}

function renderConnectionMenu() {
  connMenuEl.innerHTML = '';
  CONNECTION_OPTIONS.forEach((opt) => {
    const btn = document.createElement('button');
    btn.type = 'button';
    btn.role = 'option';
    const isSel = opt.value === selectedChunks;
    btn.className = isSel ? 'cselect-item selected' : 'cselect-item';
    btn.setAttribute('aria-selected', String(isSel));
    btn.innerHTML = `<span>${opt.label}</span>${isSel ? CHECK_SVG : ''}`;
    btn.addEventListener('click', (e) => {
      e.stopPropagation();
      selectedChunks = opt.value;
      connLabelEl.textContent = opt.label;
      renderConnectionMenu();
      closeAllMenus();
    });
    connMenuEl.appendChild(btn);
  });
}

function setSelectedCategory(cat) {
  selectedCategory = cat;
  catLabelEl.textContent = cat;
  rememberPathLabelEl.textContent = `Remember path for "${cat}"`;
  renderCategoryMenu();

  const savedDir = categoryPaths[cat];
  if (savedDir) {
    saveDirEl.value = savedDir;
    rememberPathEl.checked = true;
  } else {
    saveDirEl.value = '';
    rememberPathEl.checked = false;
  }
}

function updatePreview(syncCategory = false) {
  const ext = getExtension(fileNameEl.value, urlInputEl.value || captureData?.url);
  const { category, icon } = classifyExtension(ext, mimeTypeEl.textContent);
  const svg = ICONS[icon] || ICONS.file;
  fileIconEl.innerHTML = svg;
  p2IconEl.innerHTML = svg;
  extBadgeEl.textContent = ext ? `.${ext.toUpperCase()}` : 'FILE';
  if (syncCategory) {
    setSelectedCategory(category);
  }
}

function probeAndApplyUrl(url) {
  if (!url) return;
  renderSizeReadout(0, true);
  chrome.runtime.sendMessage({ type: 'PROBE_URL', url }, (resp) => {
    if (resp?.ok && resp.data) {
      const { content_length, content_type, file_name } = resp.data;
      if (content_length && content_length > 0) {
        if (captureData) captureData.totalBytes = content_length;
        renderSizeReadout(content_length, false);
      } else {
        renderSizeReadout(captureData?.totalBytes || 0, false);
      }
      if (content_type) {
        mimeTypeEl.textContent = resolveMimeDisplay(
          content_type,
          getExtension(fileNameEl.value, url),
        );
      }
      if (file_name && !fileNameEl.value.trim()) {
        fileNameEl.value = file_name;
        updatePreview(true);
      }
    } else {
      renderSizeReadout(captureData?.totalBytes || 0, false);
    }
  });
}

async function loadSavedPrefs() {
  const stored = await chrome.storage.local.get({
    categoryPaths: {},
    customCategories: [],
    completionOptions: { openFile: false, openFolder: false, closeDialog: false, shutdown: false },
  });
  categoryPaths = stored.categoryPaths || {};
  customCategories = Array.isArray(stored.customCategories) ? stored.customCategories : [];
  renderCategoryMenu();
  renderConnectionMenu();

  const opts = stored.completionOptions || {};
  optOpenFile.checked = Boolean(opts.openFile);
  optOpenFolder.checked = Boolean(opts.openFolder);
  optCloseDialog.checked = Boolean(opts.closeDialog);
  optShutdown.checked = Boolean(opts.shutdown);
}

function saveCompletionOptions() {
  chrome.storage.local.set({
    completionOptions: {
      openFile: optOpenFile.checked,
      openFolder: optOpenFolder.checked,
      closeDialog: optCloseDialog.checked,
      shutdown: optShutdown.checked,
    },
  });
}

async function loadCapture() {
  await loadSavedPrefs();
  if (!captureId) {
    showP1Error('Missing download capture data.');
    btnStart.disabled = true;
    renderSizeReadout(0, false);
    return;
  }

  const key = `capture_${captureId}`;
  const stored = await chrome.storage.session.get(key);
  captureData = stored[key];
  if (!captureData) {
    showP1Error('Download session expired.');
    btnStart.disabled = true;
    renderSizeReadout(0, false);
    return;
  }

  const initialName = captureData.filename || captureData.title || '';
  fileNameEl.value = initialName;
  urlInputEl.value = captureData.url || '';
  urlInputEl.title = captureData.url || '';

  const ext = getExtension(initialName, captureData.url);
  const initialMime = resolveMimeDisplay(captureData.kind, ext);
  mimeTypeEl.textContent = initialMime;

  const { category } = classifyExtension(ext, initialMime);
  setSelectedCategory(category);
  updatePreview(false);

  renderSizeReadout(captureData.totalBytes, !(captureData.totalBytes > 0));

  // Sync defaults (download folder & connection count) from the running danDown app
  fetch(`${BRIDGE_BASE}/defaults`)
    .then((r) => (r.ok ? r.json() : null))
    .then((defs) => {
      if (!defs) return;
      if (!saveDirEl.value.trim() && defs.default_download_dir) {
        saveDirEl.value = defs.default_download_dir;
      }
      if (defs.chunk_count) {
        const match = CONNECTION_OPTIONS.find((o) => o.value === defs.chunk_count);
        if (match) {
          selectedChunks = match.value;
          connLabelEl.textContent = match.label;
          renderConnectionMenu();
        }
      }
    })
    .catch(() => {})
    .finally(() => {
      fitWindowToContent(580, false);
    });

  probeAndApplyUrl(captureData.url);
}

// =========================================================================
// Single-Series Chart Math (MA5, warm-up clamp, eased visible Y-max, full-width X)
// =========================================================================

function smoothMA5(arr) {
  const out = [];
  for (let i = 0; i < arr.length; i += 1) {
    const start = Math.max(0, i - 4);
    let sum = 0;
    for (let j = start; j <= i; j += 1) sum += arr[j];
    out.push(sum / (i - start + 1));
  }
  return out;
}

function updateTargetYMax(smoothed) {
  const validStart =
    totalSampleCount > WARMUP_SAMPLES
      ? Math.max(0, WARMUP_SAMPLES - (totalSampleCount - smoothed.length))
      : 0;
  let visibleMax = 0;
  for (let i = validStart; i < smoothed.length; i += 1) {
    if (smoothed[i] > visibleMax) visibleMax = smoothed[i];
  }
  if (visibleMax === 0 && smoothed.length > 0) {
    visibleMax = Math.max(...smoothed);
  }
  targetYMax = Math.max(visibleMax * 1.2, 64 * 1024);

  if (!yAnimFrame) {
    const startVal = displayedYMax;
    const endVal = targetYMax;
    const startTs = performance.now();
    const duration = 300;
    const step = (now) => {
      const t = Math.min(1, (now - startTs) / duration);
      const eased = 1 - Math.pow(1 - t, 3);
      displayedYMax = startVal + (endVal - startVal) * eased;
      drawChartSvg(lastStatus);
      if (t < 1) {
        yAnimFrame = requestAnimationFrame(step);
      } else {
        displayedYMax = endVal;
        yAnimFrame = null;
      }
    };
    yAnimFrame = requestAnimationFrame(step);
  }
}

function buildPath(smoothed, width, height, yMax) {
  if (smoothed.length === 0) return { line: '', area: '' };
  const padTop = 8;
  const padBottom = 6;
  const usableH = height - padTop - padBottom;
  const n = smoothed.length;

  const coords = smoothed.map((val, i) => {
    const x = n === 1 ? width : (i / (n - 1)) * width;
    const y = padTop + usableH * (1 - Math.min(1, val / yMax));
    return [x, y];
  });

  if (coords.length === 1) {
    const [, y] = coords[0];
    return {
      line: `M 0 ${y.toFixed(1)} L ${width} ${y.toFixed(1)}`,
      area: `M 0 ${height} L 0 ${y.toFixed(1)} L ${width} ${y.toFixed(1)} L ${width} ${height} Z`,
    };
  }

  let line = `M ${coords[0][0].toFixed(1)} ${coords[0][1].toFixed(1)}`;
  for (let i = 1; i < coords.length; i += 1) {
    const [x0, y0] = coords[i - 1];
    const [x1, y1] = coords[i];
    const cx = ((x0 + x1) / 2).toFixed(1);
    line += ` C ${cx} ${y0.toFixed(1)}, ${cx} ${y1.toFixed(1)}, ${x1.toFixed(1)} ${y1.toFixed(1)}`;
  }
  return { line, area: `${line} L ${width} ${height} L 0 ${height} Z` };
}

function drawChartSvg(status) {
  const width = 560;
  const height = 96;
  const isPaused = status === 'paused';

  const yMax = Math.max(displayedYMax, 1024);
  const smoothed = smoothMA5(rawNet);
  const paths = buildPath(smoothed, width, height, yMax);

  netLineEl.setAttribute('d', paths.line);
  netAreaEl.setAttribute('d', paths.area);

  if (isPaused) {
    netLineEl.setAttribute('stroke', '#5f7396');
    netAreaEl.setAttribute('fill', 'url(#netGradPaused)');
  } else {
    netLineEl.setAttribute('stroke', '#22c55e');
    netAreaEl.setAttribute('fill', 'url(#netGradActive)');
  }
}

// =========================================================================
// Popup 2 Layout & State Rendering
// =========================================================================

function setActiveTab(tab) {
  const isStatus = tab === 'status';
  tabStatusBtn.classList.toggle('active', isStatus);
  tabStatusBtn.setAttribute('aria-selected', String(isStatus));
  tabOptionsBtn.classList.toggle('active', !isStatus);
  tabOptionsBtn.setAttribute('aria-selected', String(!isStatus));
  panelStatus.classList.toggle('hidden', !isStatus);
  panelOptions.classList.toggle('hidden', isStatus);
  fitWindowToContent(640, false);
}

function enterProgressView(downloadId) {
  activeDownloadId = downloadId;
  closeAllMenus();
  setupView.classList.add('hidden');
  progressView.classList.remove('hidden');

  const currentUrl = urlInputEl.value.trim() || captureData?.url || '';
  const initialName = fileNameEl.value.trim() || captureData?.filename || 'Downloading…';
  p2FilenameEl.textContent = initialName;
  p2UrlEl.textContent = currentUrl;
  p2UrlEl.title = `${currentUrl} (Click to copy)`;
  document.title = `0% — ${initialName} — danDown`;

  if (captureData?.totalBytes > 0) {
    cachedState.total = captureData.totalBytes;
    uibrikSubEl.textContent = `0.0 of ${(captureData.totalBytes / (1024 * 1024)).toFixed(1)} MB`;
  }

  fitWindowToContent(640, true);
  pollProgress();
  pollTimer = setInterval(pollProgress, POLL_INTERVAL_MS);
}

function stopPolling() {
  if (pollTimer) {
    clearInterval(pollTimer);
    pollTimer = null;
  }
}

function renderSegmentsAndTable(chunks, status) {
  const list = Array.isArray(chunks) && chunks.length > 0 ? chunks : [];
  if (list.length === 0) return;

  if (segmentsBarEl.children.length !== list.length) {
    segmentsBarEl.innerHTML = '';
    for (let i = 0; i < list.length; i += 1) {
      const track = document.createElement('div');
      track.className = 'seg-track';
      const fill = document.createElement('div');
      fill.className = 'seg-fill';
      track.appendChild(fill);
      segmentsBarEl.appendChild(track);
    }
  }

  const isCompleted = status === 'completed';
  const isPaused = status === 'paused';
  const isFailed = status === 'failed';
  let activeCount = 0;

  list.forEach((seg, idx) => {
    const fill = segmentsBarEl.children[idx]?.firstElementChild;
    if (!fill) return;
    const assigned = seg.total_bytes || 0;
    const downloaded = seg.downloaded_bytes || 0;
    const pct =
      isCompleted || seg.status === 'Done'
        ? 100
        : assigned > 0
          ? Math.min(100, (downloaded / assigned) * 100)
          : 0;

    fill.style.width = `${pct.toFixed(1)}%`;
    if (isCompleted || seg.status === 'Done') {
      fill.className = 'seg-fill completed';
    } else if (isPaused || seg.status === 'Paused') {
      fill.className = 'seg-fill paused';
    } else if (isFailed || seg.status === 'Disconnected') {
      fill.className = 'seg-fill failed';
    } else {
      fill.className = 'seg-fill active';
      activeCount += 1;
    }
  });

  segmentsSummaryEl.textContent = isCompleted
    ? 'Completed'
    : isPaused
      ? `${list.length} paused`
      : `${activeCount} of ${list.length} active`;

  connTbodyEl.innerHTML = list
    .map((seg) => {
      const stLower = (seg.status || 'Connecting').toLowerCase();
      const dotCls =
        stLower === 'done'
          ? 'done'
          : stLower === 'paused'
            ? 'paused'
            : stLower === 'disconnected'
              ? 'disconnected'
              : stLower === 'connecting'
                ? 'connecting'
                : '';
      return `<tr>
        <td class="num">${seg.index}</td>
        <td class="num">${formatBytes(seg.downloaded_bytes)}</td>
        <td><span class="conn-status"><span class="conn-dot ${dotCls}"></span>${seg.status}</span></td>
      </tr>`;
    })
    .join('');
}

function triggerCompletionActions() {
  if (completionHandled || activeDownloadId == null) return;
  completionHandled = true;

  if (optOpenFile.checked) {
    chrome.runtime.sendMessage({
      type: 'CONTROL_DOWNLOAD',
      downloadId: activeDownloadId,
      action: 'open-file',
    });
  }
  if (optOpenFolder.checked) {
    chrome.runtime.sendMessage({
      type: 'CONTROL_DOWNLOAD',
      downloadId: activeDownloadId,
      action: 'open-folder',
    });
  }
  if (optCloseDialog.checked) {
    setTimeout(() => window.close(), 600);
  }
}

function renderProgress(data) {
  const status = (data.status || 'downloading').toLowerCase();
  lastStatus = status;

  const isDownloading = status === 'downloading' || status === 'trying';
  const isPaused = status === 'paused';
  const isCompleted = status === 'completed';
  const isFailed = status === 'failed';

  const incomingTotal = data.total_bytes || captureData?.totalBytes || 0;
  if (incomingTotal > 0) cachedState.total = incomingTotal;
  const total = cachedState.total;

  const incomingDownloaded = data.downloaded_bytes || 0;
  if (incomingDownloaded >= cachedState.downloaded || isCompleted) {
    cachedState.downloaded = isCompleted && total > 0 ? total : incomingDownloaded;
  }
  const downloaded = cachedState.downloaded;

  const pct = isCompleted ? 100 : total > 0 ? Math.min(100, (downloaded / total) * 100) : 0;

  if (isDownloading) {
    cachedState.speedBps = Math.max(0, data.speed_bps || 0);
    cachedState.etaSecs = data.eta_secs || 0;
  }
  cachedState.supportsRange = data.supports_range ?? cachedState.supportsRange;
  cachedState.chunkCount = data.chunk_count || cachedState.chunkCount;

  const hasChunkProgress =
    Array.isArray(data.chunks) &&
    data.chunks.length > 0 &&
    (data.chunks.some((c) => (c.downloaded_bytes || 0) > 0) || cachedState.chunks.length === 0);
  if (hasChunkProgress) {
    cachedState.chunks = data.chunks;
  } else if (isPaused && cachedState.chunks.length > 0) {
    cachedState.chunks = cachedState.chunks.map((c) => ({
      ...c,
      status: c.status === 'Done' ? 'Done' : 'Paused',
    }));
  }

  const speedBps = isPaused ? cachedState.speedBps : isDownloading ? cachedState.speedBps : 0;
  const etaSecs = isPaused ? cachedState.etaSecs : isDownloading ? cachedState.etaSecs : 0;

  if (isDownloading) {
    rawNet.push(speedBps);
    totalSampleCount += 1;
    if (rawNet.length > WINDOW_SAMPLES) rawNet.shift();

    const smoothed = smoothMA5(rawNet);
    if (totalSampleCount > WARMUP_SAMPLES) {
      const latestSmoothed = smoothed[smoothed.length - 1] || 0;
      if (latestSmoothed > allTimePeakBps) allTimePeakBps = latestSmoothed;
    } else if (smoothed.length > 0 && allTimePeakBps === 0) {
      allTimePeakBps = smoothed[smoothed.length - 1];
    }
    updateTargetYMax(smoothed);
    drawChartSvg(status);
  } else {
    drawChartSvg(status);
  }

  const name = data.file_name || p2FilenameEl.textContent;
  p2FilenameEl.textContent = name;
  p2UrlEl.textContent = data.url || p2UrlEl.textContent;
  document.title = isCompleted
    ? `Completed — ${name} — danDown`
    : isPaused
      ? `Paused (${pct.toFixed(0)}%) — ${name} — danDown`
      : `${pct.toFixed(0)}% — ${name} — danDown`;

  p2StatusTextEl.textContent = isFailed ? 'Error' : status;
  p2StatusBadgeEl.className = `status-badge ${status}`;

  const curMB = (downloaded / (1024 * 1024)).toFixed(1);
  const totMB = (total / (1024 * 1024)).toFixed(1);
  const isFinishingSubSecond =
    isDownloading && speedBps > 0 && total > downloaded && (!etaSecs || etaSecs <= 0);
  const etaLabel = isCompleted ? 'Done' : formatEta(etaSecs, isFinishingSubSecond);

  if (total > 0) {
    const suffix =
      isDownloading && etaLabel !== '--' ? ` · ${etaLabel} left` : isPaused ? ' · Paused' : '';
    uibrikSubEl.textContent = `${curMB} of ${totMB} MB${suffix}`;
  } else {
    uibrikSubEl.textContent = `${curMB} MB`;
  }

  p2BarFillEl.style.width = `${pct.toFixed(2)}%`;
  p2BarFillEl.className = isCompleted
    ? 'progress-fill completed'
    : isFailed
      ? 'progress-fill failed'
      : isPaused
        ? 'progress-fill paused'
        : 'progress-fill active';
  p2BarFillEl.parentElement.setAttribute('aria-valuenow', String(Math.round(pct)));

  if (isCompleted) {
    p2BarPctEl.className = 'progress-pct num completed';
    p2BarPctEl.innerHTML = `<svg width="13" height="13" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M2.6 6.3 4.9 8.6 9.4 3.6"/></svg>Done`;
  } else {
    p2BarPctEl.className = 'progress-pct num';
    p2BarPctEl.textContent = `${pct.toFixed(1)}%`;
  }

  statSpeedEl.textContent = isPaused ? `${formatSpeed(speedBps)} (paused)` : formatSpeed(speedBps);
  statSpeedEl.className = isPaused ? 'stat-value num muted' : 'stat-value num';
  statEtaEl.textContent = etaLabel;
  statEtaEl.className = isPaused ? 'stat-value num muted' : 'stat-value num';
  statResumeEl.textContent = cachedState.supportsRange ? 'Yes' : 'No';
  statResumeDotEl.className = cachedState.supportsRange ? 'resume-dot' : 'resume-dot no';
  statConnsEl.textContent = String(cachedState.chunkCount || selectedChunks || 8);

  chartPeakEl.textContent = formatSpeed(allTimePeakBps);
  chartCurrentSpeedEl.textContent = isPaused
    ? formatSpeed(speedBps)
    : isCompleted
      ? 'Complete'
      : formatSpeed(speedBps);
  chartCurrentSpeedEl.className = isPaused
    ? 'chart-speed num paused'
    : isFailed
      ? 'chart-speed num failed'
      : 'chart-speed num';

  renderSegmentsAndTable(cachedState.chunks, status);

  const hadError = !p2ErrorEl.classList.contains('hidden');
  if (data.error_message) {
    p2ErrorEl.textContent = data.error_message;
    p2ErrorEl.classList.remove('hidden');
    if (!hadError) fitWindowToContent(640, false);
  } else {
    p2ErrorEl.classList.add('hidden');
    if (hadError) fitWindowToContent(640, false);
  }

  if (isCompleted) {
    btnPrimaryAction.textContent = 'Open file';
    btnPrimaryAction.disabled = false;
    btnOpenFolder.classList.remove('hidden');
    btnProgCancel.textContent = 'Close';
    btnProgCancel.className = 'btn-ghost';
    stopPolling();
    triggerCompletionActions();
  } else {
    btnOpenFolder.classList.add('hidden');
    btnProgCancel.textContent = 'Cancel';
    btnProgCancel.className = 'btn-danger-hover';
    btnPrimaryAction.textContent = isPaused || isFailed ? 'Resume' : 'Pause';
    btnPrimaryAction.disabled = false;
    if (isFailed) stopPolling();
  }
}

function pollProgress() {
  if (activeDownloadId == null) return;
  chrome.runtime.sendMessage(
    { type: 'GET_DOWNLOAD_STATUS', downloadId: activeDownloadId },
    (resp) => {
      if (resp?.ok && resp.data) {
        renderProgress(resp.data);
      } else if (resp?.notFound) {
        stopPolling();
        window.close();
      }
    },
  );
}

function startDownloadFlow(startPaused, retriedAfterWake = false) {
  if (!captureData) return;
  const targetUrl = urlInputEl.value.trim() || captureData.url;
  if (!/^https?:\/\//i.test(targetUrl) && !/^magnet:/i.test(targetUrl)) {
    showP1Error('Enter a valid http:// or https:// download URL.');
    urlInputEl.focus();
    return;
  }

  btnStart.disabled = true;
  btnLater.disabled = true;
  btnStart.textContent = startPaused ? 'Queueing…' : 'Starting…';
  showP1Error('');

  const dir = saveDirEl.value.trim();
  const file = fileNameEl.value.trim();
  if (rememberPathEl.checked && dir) {
    categoryPaths[selectedCategory] = dir;
    chrome.storage.local.set({ categoryPaths });
  }

  chrome.runtime.sendMessage(
    {
      type: 'CONFIRM_DOWNLOAD',
      captureId,
      url: targetUrl,
      referrer: captureData.referrer,
      title: file || captureData.filename || captureData.title || undefined,
      filePath: dir || undefined,
      chunkCount: selectedChunks || 8,
      startPaused,
    },
    (resp) => {
      if (resp?.ok) {
        if (!startPaused && resp.downloadId != null) {
          enterProgressView(resp.downloadId);
        } else {
          window.close();
        }
      } else if (!retriedAfterWake) {
        btnStart.textContent = 'Waking danDown…';
        chrome.runtime.sendMessage({ type: 'WAKE_DANDOWN' }, (wakeResp) => {
          if (wakeResp?.ok) {
            startDownloadFlow(startPaused, true);
          } else {
            btnStart.disabled = false;
            btnLater.disabled = false;
            btnStart.textContent = 'Start download';
            showP1Error(
              resp?.error ||
                'Could not reach danDown. Make sure the desktop app is installed/running.',
            );
          }
        });
      } else {
        btnStart.disabled = false;
        btnLater.disabled = false;
        btnStart.textContent = 'Start download';
        showP1Error(
          resp?.error || 'Could not reach danDown. Make sure the desktop app is running.',
        );
      }
    },
  );
}

// =========================================================================
// Event Listeners
// =========================================================================

fileNameEl.addEventListener('input', () => updatePreview(false));

urlInputEl.addEventListener('change', () => {
  const edited = urlInputEl.value.trim();
  if (!edited || !captureData) return;
  captureData.url = edited;
  urlInputEl.title = edited;
  updatePreview(false);
  probeAndApplyUrl(edited);
});

// Custom dropdown toggles
catTrigger.addEventListener('click', (e) => {
  e.stopPropagation();
  const willOpen = catMenuEl.classList.contains('hidden');
  closeAllMenus();
  if (willOpen) {
    catMenuEl.classList.remove('hidden');
    catTrigger.setAttribute('aria-expanded', 'true');
  }
});

connTrigger.addEventListener('click', (e) => {
  e.stopPropagation();
  const willOpen = connMenuEl.classList.contains('hidden');
  closeAllMenus();
  if (willOpen) {
    connMenuEl.classList.remove('hidden');
    connTrigger.setAttribute('aria-expanded', 'true');
  }
});

document.addEventListener('click', closeAllMenus);

// Native OS Folder Picker directly via danDown bridge (avoids MV3 service worker 30s timeout)
btnBrowse.addEventListener('click', async () => {
  btnBrowse.disabled = true;
  showP1Error('');
  try {
    const res = await fetch(
      `${BRIDGE_BASE}/pick-folder?dir=${encodeURIComponent(saveDirEl.value.trim())}`,
      { method: 'POST' },
    );
    const data = await res.json().catch(() => null);
    if (res.ok && data?.ok && data.path) {
      saveDirEl.value = data.path;
    } else if (!res.ok) {
      showP1Error(data?.message || `Folder picker error (HTTP ${res.status})`);
    }
  } catch {
    showP1Error('Could not reach danDown to open folder picker.');
  } finally {
    btnBrowse.disabled = false;
  }
});

// Inline custom category adder (replaces browser prompt)
btnAddCategory.addEventListener('click', () => {
  const opening = newCatRowEl.classList.contains('hidden');
  newCatRowEl.classList.toggle('hidden', !opening);
  fitWindowToContent(580, false);
  if (opening) {
    newCatInputEl.value = '';
    newCatInputEl.focus();
  }
});

function commitNewCategory() {
  const trimmed = newCatInputEl.value.trim();
  if (!trimmed) {
    newCatRowEl.classList.add('hidden');
    fitWindowToContent(580, false);
    return;
  }
  const all = [...DEFAULT_CATEGORIES, ...customCategories];
  if (!all.some((c) => c.toLowerCase() === trimmed.toLowerCase())) {
    customCategories.push(trimmed);
    chrome.storage.local.set({ customCategories });
  }
  setSelectedCategory(trimmed);
  newCatRowEl.classList.add('hidden');
  fitWindowToContent(580, false);
}

btnSaveCat.addEventListener('click', commitNewCategory);
btnCancelCat.addEventListener('click', () => {
  newCatRowEl.classList.add('hidden');
  fitWindowToContent(580, false);
});
newCatInputEl.addEventListener('keydown', (e) => {
  if (e.key === 'Enter') {
    e.preventDefault();
    e.stopPropagation();
    commitNewCategory();
  } else if (e.key === 'Escape') {
    e.preventDefault();
    e.stopPropagation();
    newCatRowEl.classList.add('hidden');
    fitWindowToContent(580, false);
  }
});

btnToggleNote.addEventListener('click', () => {
  const show = noteRowEl.classList.contains('hidden');
  noteRowEl.classList.toggle('hidden', !show);
  btnToggleNote.textContent = show ? 'Hide note' : '+ Add note';
  fitWindowToContent(580, false);
  if (show) descriptionEl.focus();
});

function copyUrlWithFeedback(text, feedbackEl) {
  if (!text) return;
  navigator.clipboard?.writeText(text).then(() => {
    feedbackEl.classList.remove('hidden');
    setTimeout(() => feedbackEl.classList.add('hidden'), 1400);
  });
}

btnCopyUrl.addEventListener('click', () =>
  copyUrlWithFeedback(urlInputEl.value.trim(), p1CopyTagEl),
);
p2UrlEl.addEventListener('click', () => copyUrlWithFeedback(p2UrlEl.textContent, copyFeedbackEl));

btnStart.addEventListener('click', () => startDownloadFlow(false));
btnLater.addEventListener('click', () => startDownloadFlow(true));

btnBrowser.addEventListener('click', () => {
  const targetUrl = urlInputEl.value.trim() || captureData?.url;
  if (!targetUrl) {
    window.close();
    return;
  }
  chrome.runtime.sendMessage({ type: 'FALLBACK_BROWSER_DOWNLOAD', captureId, url: targetUrl }, () =>
    window.close(),
  );
});

btnCancel.addEventListener('click', () => {
  if (captureId) chrome.storage.session.remove(`capture_${captureId}`);
  window.close();
});

tabStatusBtn.addEventListener('click', () => setActiveTab('status'));
tabOptionsBtn.addEventListener('click', () => setActiveTab('options'));

function togglePauseResume() {
  if (activeDownloadId == null) return;
  if (lastStatus === 'completed') {
    chrome.runtime.sendMessage(
      { type: 'CONTROL_DOWNLOAD', downloadId: activeDownloadId, action: 'open-file' },
      () => window.close(),
    );
    return;
  }
  const action = lastStatus === 'paused' || lastStatus === 'failed' ? 'resume' : 'pause';
  chrome.runtime.sendMessage(
    { type: 'CONTROL_DOWNLOAD', downloadId: activeDownloadId, action },
    () => {
      if (action === 'resume' && !pollTimer) {
        pollTimer = setInterval(pollProgress, POLL_INTERVAL_MS);
      }
      pollProgress();
    },
  );
}

btnPrimaryAction.addEventListener('click', togglePauseResume);

btnOpenFolder.addEventListener('click', () => {
  if (activeDownloadId == null) return;
  chrome.runtime.sendMessage(
    { type: 'CONTROL_DOWNLOAD', downloadId: activeDownloadId, action: 'open-folder' },
    () => window.close(),
  );
});

btnToggleDetails.addEventListener('click', () => {
  detailsOpen = !detailsOpen;
  detailsPanelEl.classList.toggle('hidden', !detailsOpen);
  btnToggleDetails.setAttribute('aria-expanded', String(detailsOpen));
  detailsBtnTextEl.textContent = detailsOpen ? 'Hide details' : 'Show details';
  detailsChevronEl.classList.toggle('open', detailsOpen);
  fitWindowToContent(640, false);
});

btnProgCancel.addEventListener('click', () => {
  if (lastStatus === 'completed' || activeDownloadId == null) {
    stopPolling();
    window.close();
    return;
  }
  stopPolling();
  chrome.runtime.sendMessage(
    { type: 'CONTROL_DOWNLOAD', downloadId: activeDownloadId, action: 'cancel' },
    () => window.close(),
  );
});

[optOpenFile, optOpenFolder, optCloseDialog, optShutdown].forEach((el) => {
  el.addEventListener('change', saveCompletionOptions);
});

document.addEventListener('keydown', (e) => {
  if (e.key === 'Escape') {
    if (!catMenuEl.classList.contains('hidden') || !connMenuEl.classList.contains('hidden')) {
      closeAllMenus();
      return;
    }
    if (activeDownloadId == null) {
      btnCancel.click();
    } else {
      stopPolling();
      window.close();
    }
    return;
  }

  if (
    e.key === 'Enter' &&
    activeDownloadId == null &&
    !btnStart.disabled &&
    document.activeElement?.tagName !== 'BUTTON' &&
    document.activeElement !== newCatInputEl
  ) {
    btnStart.click();
  }
});

window.addEventListener('beforeunload', () => {
  stopPolling();
  if (yAnimFrame) cancelAnimationFrame(yAnimFrame);
});

loadCapture();
