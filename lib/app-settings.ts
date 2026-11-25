export type BuilderPreset = 'default' | 'high-throughput' | 'low-cpu' | 'satellite' | 'custom';
export type DownloadEngine = 'native' | 'aria2';

export interface AppSettings {
  preset: BuilderPreset;
  defaultEngine: DownloadEngine;
  aria2Path: string;
  chunkCount: number;
  maxRetries: number;
  backoffFactor: number;
  timeoutSecs: number;
  defaultDownloadDir: string;
  clipboardMonitor: boolean;
}

export const PRESET_CONFIGS: Record<
  Exclude<BuilderPreset, 'custom'>,
  Pick<AppSettings, 'chunkCount' | 'maxRetries' | 'backoffFactor' | 'timeoutSecs'>
> = {
  default: {
    chunkCount: 8,
    maxRetries: 3,
    backoffFactor: 2,
    timeoutSecs: 30,
  },
  'high-throughput': {
    chunkCount: 16,
    maxRetries: 5,
    backoffFactor: 2,
    timeoutSecs: 45,
  },
  'low-cpu': {
    chunkCount: 2,
    maxRetries: 3,
    backoffFactor: 2,
    timeoutSecs: 30,
  },
  satellite: {
    chunkCount: 8,
    maxRetries: 10,
    backoffFactor: 3,
    timeoutSecs: 90,
  },
};

export const DEFAULT_APP_SETTINGS: AppSettings = {
  preset: 'default',
  defaultEngine: 'aria2',
  aria2Path: '',
  ...PRESET_CONFIGS.default,
  defaultDownloadDir: '',
  clipboardMonitor: true,
};

const STORAGE_KEY = 'danDown_app_settings_v1';
const SETTINGS_EVENT = 'danDown-settings-changed';

export function getAppSettings(): AppSettings {
  if (typeof window === 'undefined') {
    return DEFAULT_APP_SETTINGS;
  }
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (!raw) {
      return DEFAULT_APP_SETTINGS;
    }
    const parsed = JSON.parse(raw) as Partial<AppSettings>;
    return {
      ...DEFAULT_APP_SETTINGS,
      ...parsed,
      chunkCount: Math.min(16, Math.max(1, Number(parsed.chunkCount ?? 8))),
    };
  } catch {
    return DEFAULT_APP_SETTINGS;
  }
}

export function saveAppSettings(next: AppSettings): void {
  if (typeof window === 'undefined') {
    return;
  }
  try {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(next));
    window.dispatchEvent(new CustomEvent(SETTINGS_EVENT));
  } catch (err) {
    console.error('Failed to save settings:', err);
  }
}

export function subscribeAppSettings(listener: () => void): () => void {
  if (typeof window === 'undefined') {
    return () => {};
  }
  window.addEventListener(SETTINGS_EVENT, listener);
  window.addEventListener('storage', listener);
  return () => {
    window.removeEventListener(SETTINGS_EVENT, listener);
    window.removeEventListener('storage', listener);
  };
}

const DOWNLOADABLE_URL_RE =
  /\.(zip|rar|7z|tar|gz|bz2|xz|iso|exe|msi|dmg|pkg|deb|rpm|appimage|apk|mp4|mkv|webm|avi|mov|mp3|flac|wav|ogg|m4a|pdf|epub|csv|json|sql|bin|img|m3u8)(\?|#|$)/i;

export function looksLikeDownloadableUrl(text: string): boolean {
  const trimmed = text.trim();
  if (!trimmed || trimmed.includes(' ') || trimmed.includes('\n')) {
    return false;
  }
  if (/^magnet:\?\S+/i.test(trimmed)) {
    return true;
  }
  try {
    const u = new URL(trimmed);
    if (!['http:', 'https:', 'ftp:', 'sftp:'].includes(u.protocol)) {
      return false;
    }
    return DOWNLOADABLE_URL_RE.test(u.pathname);
  } catch {
    return false;
  }
}

export function extractUrlsFromText(text: string): string[] {
  const matches = text.match(/https?:\/\/[^\s"'<>]+/gi) ?? [];
  const unique = new Set<string>();
  for (const raw of matches) {
    const cleaned = raw.replace(/[),.;]+$/, '');
    try {
      const parsed = new URL(cleaned);
      if (parsed.protocol === 'http:' || parsed.protocol === 'https:') {
        unique.add(parsed.toString());
      }
    } catch {
      // ignore malformed URL tokens
    }
  }
  return Array.from(unique);
}
