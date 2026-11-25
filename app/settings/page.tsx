'use client';

import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import {
  CheckCircle2,
  FolderOpen,
  Globe,
  Monitor,
  Moon,
  RefreshCw,
  Sun,
  XCircle,
} from 'lucide-react';
import { useTheme } from 'next-themes';
import { useCallback, useEffect, useState } from 'react';
import { toast } from 'sonner';

import {
  Accordion,
  AccordionContent,
  AccordionItem,
  AccordionTrigger,
} from '@/components/ui/accordion';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { Slider } from '@/components/ui/slider';
import { Switch } from '@/components/ui/switch';
import {
  DEFAULT_APP_SETTINGS,
  getAppSettings,
  PRESET_CONFIGS,
  saveAppSettings,
} from '@/lib/app-settings';

import type { AppSettings, BuilderPreset, DownloadEngine } from '@/lib/app-settings';

interface Aria2StatusInfo {
  available: boolean;
  binary_path: string | null;
  version: string | null;
  rpc_running: boolean;
  message: string;
}

export default function SettingsPage() {
  const { theme, setTheme } = useTheme();
  const [settings, setSettings] = useState<AppSettings>(DEFAULT_APP_SETTINGS);
  const [bridgeOnline, setBridgeOnline] = useState<boolean | null>(null);
  const [aria2Status, setAria2Status] = useState<Aria2StatusInfo | null>(null);
  const [checkingAria2, setCheckingAria2] = useState(false);
  const [globalMaxConcurrent, setGlobalMaxConcurrent] = useState('5');
  const [globalSpeedLimitMb, setGlobalSpeedLimitMb] = useState('');

  const refreshAria2Status = useCallback(async (customPath: string) => {
    setCheckingAria2(true);
    try {
      const info = await invoke<Aria2StatusInfo>('check_aria2_status', {
        customPath: customPath || null,
      });
      setAria2Status(info);
    } catch {
      setAria2Status(null);
    } finally {
      setCheckingAria2(false);
    }
  }, []);

  useEffect(() => {
    const loaded = getAppSettings();
    setSettings(loaded);
    refreshAria2Status(loaded.aria2Path);
  }, [refreshAria2Status]);

  useEffect(() => {
    let active = true;
    fetch('http://127.0.0.1:17890/ping')
      .then((r) => {
        if (active) {
          setBridgeOnline(r.ok);
        }
        return null;
      })
      .catch(() => {
        if (active) {
          setBridgeOnline(false);
        }
      });
    return () => {
      active = false;
    };
  }, []);

  const updateSettings = (patch: Partial<AppSettings>) => {
    setSettings((prev) => {
      const next = { ...prev, ...patch };
      saveAppSettings(next);
      return next;
    });
  };

  const handlePresetChange = (preset: BuilderPreset) => {
    if (preset === 'custom') {
      updateSettings({ preset });
      return;
    }
    updateSettings({
      preset,
      ...PRESET_CONFIGS[preset],
    });
    toast.success(`Applied "${preset}" profile`);
  };

  const handleChooseDefaultDir = async () => {
    try {
      const selected = await open({
        directory: true,
        multiple: false,
        title: 'Select Default Download Folder',
      });
      if (typeof selected === 'string') {
        updateSettings({ defaultDownloadDir: selected });
      }
    } catch (err) {
      console.error('Failed to pick folder:', err);
    }
  };

  const handleChooseAria2Binary = async () => {
    try {
      const selected = await open({
        directory: false,
        multiple: false,
        title: 'Select aria2c Executable',
      });
      if (typeof selected === 'string') {
        updateSettings({ aria2Path: selected });
        refreshAria2Status(selected);
      }
    } catch (err) {
      console.error('Failed to pick aria2c executable:', err);
    }
  };

  const handleApplyGlobalAria2Options = async () => {
    const concurrent = Math.max(1, Math.min(16, Number.parseInt(globalMaxConcurrent, 10) || 5));
    const speedMb = Number.parseFloat(globalSpeedLimitMb);
    const limitValue =
      Number.isFinite(speedMb) && speedMb > 0 ? `${Math.round(speedMb * 1024)}K` : '0';

    try {
      await invoke('engine_change_global_option', {
        options: {
          'max-concurrent-downloads': String(concurrent),
          'max-overall-download-limit': limitValue,
        },
      });
      toast.success('Updated live aria2 engine limits');
    } catch (err) {
      toast.error('Could not update aria2 limits', { description: String(err) });
    }
  };

  return (
    <div className="m-4 space-y-4">
      <header>
        <h1 className="text-2xl font-bold tracking-tight">Settings</h1>
        <p className="text-muted-foreground mt-2 text-sm">
          Configure appearance, default multi-chunk download profiles, clipboard capture, and the
          browser extension bridge.
        </p>
      </header>

      <Accordion
        type="multiple"
        className="w-full"
        defaultValue={['appearance', 'download-engine', 'download-builder', 'browser-extension']}
      >
        <AccordionItem value="download-engine">
          <AccordionTrigger>
            <div className="flex items-center gap-2">
              <span>Download Engine</span>
              <Badge variant="secondary">
                {settings.defaultEngine === 'aria2' ? 'aria2' : 'Native Rust'}
              </Badge>
              {aria2Status?.available && (
                <Badge variant="secondary" className="gap-1 text-emerald-500">
                  <CheckCircle2 className="h-3 w-3" />
                  aria2c ready
                </Badge>
              )}
            </div>
          </AccordionTrigger>
          <AccordionContent className="flex flex-col gap-4">
            <div className="grid gap-4 sm:grid-cols-2">
              <div className="space-y-2">
                <Label htmlFor="default-engine">Default Engine</Label>
                <Select
                  value={settings.defaultEngine}
                  onValueChange={(v) => updateSettings({ defaultEngine: v as DownloadEngine })}
                >
                  <SelectTrigger id="default-engine">
                    <SelectValue placeholder="Select engine" />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="aria2">aria2 (Default — HTTP/FTP/SFTP/Magnet)</SelectItem>
                    <SelectItem value="native">Native Rust Engine</SelectItem>
                  </SelectContent>
                </Select>
                <p className="text-muted-foreground text-xs">
                  Magnet, .torrent, FTP, and SFTP links automatically route through aria2 even when
                  Native is default.
                </p>
              </div>

              <div className="space-y-2">
                <Label htmlFor="aria2-path">aria2c Binary Path (optional)</Label>
                <div className="flex gap-2">
                  <Input
                    id="aria2-path"
                    value={settings.aria2Path}
                    onChange={(e) => updateSettings({ aria2Path: e.target.value })}
                    placeholder={aria2Status?.binary_path || 'Auto-detect from PATH'}
                  />
                  <Button type="button" variant="outline" onClick={handleChooseAria2Binary}>
                    <FolderOpen className="h-4 w-4" />
                  </Button>
                  <Button
                    type="button"
                    variant="outline"
                    onClick={() => refreshAria2Status(settings.aria2Path)}
                    disabled={checkingAria2}
                    title="Re-check aria2c"
                  >
                    <RefreshCw className={`h-4 w-4 ${checkingAria2 ? 'animate-spin' : ''}`} />
                  </Button>
                </div>
                {aria2Status && (
                  <p
                    className={`text-xs ${aria2Status.available ? 'text-emerald-500' : 'text-amber-500'}`}
                  >
                    {aria2Status.version ? `${aria2Status.version} — ` : ''}
                    {aria2Status.message}
                  </p>
                )}
              </div>
            </div>

            <div className="grid gap-4 border-t pt-3 sm:grid-cols-3">
              <div className="space-y-1.5">
                <Label htmlFor="global-concurrent">Max Concurrent Downloads</Label>
                <Input
                  id="global-concurrent"
                  type="number"
                  min={1}
                  max={16}
                  value={globalMaxConcurrent}
                  onChange={(e) => setGlobalMaxConcurrent(e.target.value)}
                />
              </div>
              <div className="space-y-1.5">
                <Label htmlFor="global-speed-cap">Global Speed Cap (MB/s, blank = unlimited)</Label>
                <Input
                  id="global-speed-cap"
                  type="number"
                  min={0}
                  step="0.5"
                  placeholder="Unlimited"
                  value={globalSpeedLimitMb}
                  onChange={(e) => setGlobalSpeedLimitMb(e.target.value)}
                />
              </div>
              <div className="flex items-end">
                <Button type="button" variant="outline" onClick={handleApplyGlobalAria2Options}>
                  Apply Live Limits
                </Button>
              </div>
            </div>
          </AccordionContent>
        </AccordionItem>
        <AccordionItem value="appearance">
          <AccordionTrigger>Appearance & Clipboard</AccordionTrigger>
          <AccordionContent className="flex flex-col gap-5">
            <div className="space-y-2">
              <Label htmlFor="theme">Theme</Label>
              <Select value={theme ?? 'system'} onValueChange={setTheme}>
                <SelectTrigger id="theme" className="w-[220px]">
                  <SelectValue placeholder="Select theme" />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="light">
                    <div className="flex items-center gap-2">
                      <Sun className="h-4 w-4" />
                      <span>Light</span>
                    </div>
                  </SelectItem>
                  <SelectItem value="dark">
                    <div className="flex items-center gap-2">
                      <Moon className="h-4 w-4" />
                      <span>Dark</span>
                    </div>
                  </SelectItem>
                  <SelectItem value="system">
                    <div className="flex items-center gap-2">
                      <Monitor className="h-4 w-4" />
                      <span>System</span>
                    </div>
                  </SelectItem>
                </SelectContent>
              </Select>
            </div>

            <div className="flex items-center justify-between rounded-lg border p-3">
              <div className="space-y-0.5">
                <Label htmlFor="clipboard-monitor">Auto-detect links from clipboard</Label>
                <p className="text-muted-foreground text-xs">
                  When danDown gains focus, automatically pre-fill copied download or .m3u8 URLs.
                </p>
              </div>
              <Switch
                id="clipboard-monitor"
                checked={settings.clipboardMonitor}
                onCheckedChange={(checked) => updateSettings({ clipboardMonitor: checked })}
              />
            </div>
          </AccordionContent>
        </AccordionItem>

        <AccordionItem value="download-builder">
          <AccordionTrigger>Default Download Builder</AccordionTrigger>
          <AccordionContent className="flex flex-col gap-4">
            <p className="text-muted-foreground text-sm">
              Choose default parameters applied to quick downloads, batch imports, and new download
              sheets.
            </p>

            <div className="grid gap-4 sm:grid-cols-2">
              <div className="space-y-2">
                <Label htmlFor="builder-preset">Profile Preset</Label>
                <Select
                  value={settings.preset}
                  onValueChange={(v) => handlePresetChange(v as BuilderPreset)}
                >
                  <SelectTrigger id="builder-preset">
                    <SelectValue placeholder="Select preset" />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="default">Default (8 chunks)</SelectItem>
                    <SelectItem value="high-throughput">High Throughput (16 chunks)</SelectItem>
                    <SelectItem value="low-cpu">Low CPU / Gentle (2 chunks)</SelectItem>
                    <SelectItem value="satellite">High Latency / Resilient</SelectItem>
                    <SelectItem value="custom">Custom</SelectItem>
                  </SelectContent>
                </Select>
              </div>

              <div className="space-y-2">
                <Label htmlFor="default-dir">Default Download Folder</Label>
                <div className="flex gap-2">
                  <Input
                    id="default-dir"
                    readOnly
                    value={settings.defaultDownloadDir}
                    placeholder="~/Downloads/danDown (system default)"
                  />
                  <Button type="button" variant="outline" onClick={handleChooseDefaultDir}>
                    <FolderOpen className="h-4 w-4" />
                  </Button>
                  {settings.defaultDownloadDir && (
                    <Button
                      type="button"
                      variant="ghost"
                      onClick={() => updateSettings({ defaultDownloadDir: '' })}
                    >
                      Reset
                    </Button>
                  )}
                </div>
              </div>
            </div>

            <div className="space-y-2">
              <div className="flex justify-between">
                <Label>Default Parallel Chunks (1–16)</Label>
                <span className="text-muted-foreground text-xs">{settings.chunkCount}</span>
              </div>
              <Slider
                min={1}
                max={16}
                step={1}
                value={[settings.chunkCount]}
                onValueChange={(v) => updateSettings({ preset: 'custom', chunkCount: v[0] })}
              />
            </div>

            <div className="grid gap-4 sm:grid-cols-2">
              <div className="space-y-2">
                <div className="flex justify-between">
                  <Label>Max Retries</Label>
                  <span className="text-muted-foreground text-xs">{settings.maxRetries}</span>
                </div>
                <Slider
                  min={0}
                  max={20}
                  step={1}
                  value={[settings.maxRetries]}
                  onValueChange={(v) => updateSettings({ preset: 'custom', maxRetries: v[0] })}
                />
              </div>

              <div className="space-y-2">
                <div className="flex justify-between">
                  <Label>Stream Timeout (secs)</Label>
                  <span className="text-muted-foreground text-xs">{settings.timeoutSecs}s</span>
                </div>
                <Slider
                  min={10}
                  max={120}
                  step={5}
                  value={[settings.timeoutSecs]}
                  onValueChange={(v) => updateSettings({ preset: 'custom', timeoutSecs: v[0] })}
                />
              </div>
            </div>
          </AccordionContent>
        </AccordionItem>

        <AccordionItem value="browser-extension">
          <AccordionTrigger>
            <div className="flex items-center gap-2">
              <span>Browser Integration</span>
              {bridgeOnline === true && (
                <Badge variant="secondary" className="gap-1 text-emerald-500">
                  <CheckCircle2 className="h-3 w-3" />
                  Bridge Active (127.0.0.1:17890)
                </Badge>
              )}
              {bridgeOnline === false && (
                <Badge variant="secondary" className="gap-1 text-amber-500">
                  <XCircle className="h-3 w-3" />
                  Bridge Offline
                </Badge>
              )}
            </div>
          </AccordionTrigger>
          <AccordionContent className="flex flex-col gap-3">
            <p className="text-muted-foreground text-sm">
              Automatically intercept downloads from Chrome, Edge, Brave, or Firefox (including
              session cookies, Referer, and User-Agent headers) using the bundled WebExtension in{' '}
              <code className="bg-muted rounded px-1.5 py-0.5 text-xs">browser-extension/</code>.
            </p>
            <ol className="text-muted-foreground list-inside list-decimal space-y-1 text-xs">
              <li>
                Open <code className="text-foreground">chrome://extensions</code> or{' '}
                <code className="text-foreground">edge://extensions</code> and enable{' '}
                <b>Developer mode</b>.
              </li>
              <li>
                Click <b>Load unpacked</b> and select the{' '}
                <code className="text-foreground">browser-extension</code> folder inside your danDown
                project directory.
              </li>
              <li>
                Toggle auto-intercept in the extension popup or right-click any link and choose{' '}
                <b>Download with danDown</b>.
              </li>
            </ol>
            <div className="flex items-center gap-2 pt-1">
              <Globe className="text-muted-foreground h-4 w-4" />
              <span className="text-muted-foreground text-xs">
                Local capture endpoint: <code>http://127.0.0.1:17890/download</code>
              </span>
            </div>
          </AccordionContent>
        </AccordionItem>
      </Accordion>
    </div>
  );
}
