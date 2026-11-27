'use client';

import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { AnimatePresence, motion } from 'framer-motion';
import { DownloadIcon, Pause, Play, Trash2, Upload } from 'lucide-react';
import { useEffect, useMemo, useRef, useState } from 'react';
import { toast } from 'sonner';

import DownloadBar from '@/components/download-bar';
import { EmptyDownloadsIntro } from '@/components/empty-download-intro';
import { Button } from '@/components/ui/button';
import { getAppSettings } from '@/lib/app-settings';

import { useDownloads } from '../components/download-context';
import DownloadItem from '../components/download-item';
import DownloadSettingSheet from '../components/download-setting/download-setting-sheet';
import { Status } from '../components/types';

const statusRank = (s: Status) => {
  switch (s) {
    case Status.Downloading:
      return 0;
    case Status.Queued:
      return 1;
    default:
      return 2;
  }
};

export default function Page() {
  const { filteredDownloads, isLoading, removeDownload } = useDownloads();
  const [isModalOpen, setIsModalOpen] = useState(false);
  const [isDraggingFiles, setIsDraggingFiles] = useState(false);
  const [url, setUrl] = useState('');

  const prevStatus = useRef<Map<number, Status>>(new Map());
  const [justPromotedId, setJustPromotedId] = useState<number | null>(null);

  useEffect(() => {
    const s = getAppSettings();
    fetch('http://127.0.0.1:17890/defaults', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        default_download_dir: s.defaultDownloadDir || null,
        chunk_count: s.chunkCount,
      }),
    }).catch(() => {});
  }, []);

  useEffect(() => {
    const unlisten = listen<string>('error', (ev) => {
      toast.error('Error', { description: ev.payload });
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  useEffect(() => {
    let cancelled = false;
    const unlistenPromise = getCurrentWebview().onDragDropEvent(async (event) => {
      if (cancelled) {
        return;
      }
      if (event.payload.type === 'enter' || event.payload.type === 'over') {
        setIsDraggingFiles(true);
      } else if (event.payload.type === 'leave') {
        setIsDraggingFiles(false);
      } else if (event.payload.type === 'drop') {
        setIsDraggingFiles(false);
        const paths = event.payload.paths;
        if (paths.length === 0) {
          return;
        }
        try {
          const count = await invoke<number>('import_dropped_files', { paths });
          if (count > 0) {
            toast.success(`Imported ${count} download${count === 1 ? '' : 's'} from dropped file`);
          } else {
            toast.info('Drop a .torrent file or .txt URL list to import downloads');
          }
        } catch (err) {
          toast.error('Failed to import dropped file', { description: String(err) });
        }
      }
    });

    return () => {
      cancelled = true;
      unlistenPromise.then((fn) => fn());
    };
  }, []);

  useEffect(() => {
    for (const d of filteredDownloads) {
      const prev = prevStatus.current.get(d.id);
      if (prev && prev !== Status.Downloading && d.status === Status.Downloading) {
        setJustPromotedId(d.id);
        setTimeout(() => {
          setJustPromotedId((x) => (x === d.id ? null : x));
        }, 1200);
      }
    }
    for (const d of filteredDownloads) {
      prevStatus.current.set(d.id, d.status);
    }
  }, [filteredDownloads]);

  const ordered = useMemo(() => {
    return [...filteredDownloads].sort((a, b) => {
      const rA = statusRank(a.status);
      const rB = statusRank(b.status);
      if (rA !== rB) return rA - rB;

      if (a.status === Status.Downloading && b.status === Status.Downloading) {
        return new Date(b.modified_at).getTime() - new Date(a.modified_at).getTime();
      }

      return new Date(b.created_at).getTime() - new Date(a.created_at).getTime();
    });
  }, [filteredDownloads]);

  const stats = useMemo(() => {
    let active = 0;
    let paused = 0;
    let completed = 0;
    for (const d of ordered) {
      if (
        d.status === Status.Downloading ||
        d.status === Status.Queued ||
        d.status === Status.Trying
      ) {
        active += 1;
      } else if (d.status === Status.Paused || d.status === Status.Failed) {
        paused += 1;
      } else if (d.status === Status.Completed) {
        completed += 1;
      }
    }
    return { active, paused, completed };
  }, [ordered]);

  const handlePauseAll = async () => {
    const targets = ordered.filter(
      (d) =>
        d.status === Status.Downloading || d.status === Status.Queued || d.status === Status.Trying,
    );
    await Promise.all(targets.map((d) => invoke('pause_download', { id: d.id })));
  };

  const handleResumeAll = async () => {
    const targets = ordered.filter((d) => d.status === Status.Paused || d.status === Status.Failed);
    await Promise.all(targets.map((d) => invoke('resume_download', { id: d.id })));
  };

  const handleClearCompleted = async () => {
    const targets = ordered.filter((d) => d.status === Status.Completed);
    await Promise.all(targets.map((d) => removeDownload(d.id, false)));
    if (targets.length > 0) {
      toast.success(`Cleared ${targets.length} completed item${targets.length === 1 ? '' : 's'}`);
    }
  };

  if (isLoading) {
    return (
      <div className="container mx-auto max-w-4xl">
        <div className="flex h-64 items-center justify-center">
          <div className="text-center">
            <DownloadIcon className="mx-auto mb-2 h-8 w-8 animate-pulse" />
            <p className="text-muted-foreground">Loading downloads...</p>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="relative container mx-auto max-w-5xl">
      {isDraggingFiles && (
        <div className="bg-background/85 fixed inset-0 z-50 flex flex-col items-center justify-center gap-3 backdrop-blur-sm">
          <div className="border-primary/60 bg-card flex flex-col items-center gap-2 rounded-xl border-2 border-dashed px-10 py-8 shadow-lg">
            <Upload className="text-primary h-10 w-10" />
            <p className="text-base font-semibold">Drop .torrent or .txt URL list</p>
            <p className="text-muted-foreground text-xs">
              danDown will automatically queue all links and torrents
            </p>
          </div>
        </div>
      )}
      <div className="bg-background sticky top-0 z-20 mb-3">
        <DownloadBar setUrl={setUrl} url={url} setIsModalOpen={setIsModalOpen} />
      </div>

      <div className="space-y-4 px-3">
        {ordered.length > 0 && (
          <div className="bg-card/60 flex flex-wrap items-center justify-between gap-2 rounded-lg border px-3 py-2 text-xs">
            <div className="text-muted-foreground flex items-center gap-3">
              <span>
                <strong className="text-foreground">{ordered.length}</strong> total
              </span>
              {stats.active > 0 && <span>{stats.active} active</span>}
              {stats.paused > 0 && <span>{stats.paused} paused</span>}
              {stats.completed > 0 && <span>{stats.completed} completed</span>}
            </div>
            <div className="flex items-center gap-1.5">
              {stats.active > 0 && (
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  className="h-7 px-2.5 text-xs"
                  onClick={handlePauseAll}
                >
                  <Pause className="mr-1.5 h-3 w-3" />
                  Pause All
                </Button>
              )}
              {stats.paused > 0 && (
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  className="h-7 px-2.5 text-xs"
                  onClick={handleResumeAll}
                >
                  <Play className="mr-1.5 h-3 w-3" />
                  Resume All
                </Button>
              )}
              {stats.completed > 0 && (
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  className="h-7 px-2.5 text-xs"
                  onClick={handleClearCompleted}
                >
                  <Trash2 className="mr-1.5 h-3 w-3" />
                  Clear Completed
                </Button>
              )}
            </div>
          </div>
        )}

        {ordered.length === 0 ? (
          <EmptyDownloadsIntro />
        ) : (
          <div className="grid grid-cols-[repeat(auto-fill,_minmax(500px,_1fr))] gap-2">
            <AnimatePresence initial={false}>
              {ordered.map((item) => {
                const highlighted = justPromotedId === item.id;
                return (
                  <motion.div
                    key={item.id}
                    layout
                    initial={highlighted ? { opacity: 0, y: 16, scale: 0.98 } : false}
                    animate={{ opacity: 1, y: 0, scale: 1 }}
                    exit={{ opacity: 0, y: -8 }}
                    transition={{
                      type: 'spring',
                      stiffness: 420,
                      damping: 34,
                      mass: 0.6,
                    }}
                    className={highlighted ? 'rounded-xl ring-2 ring-blue-500/40' : ''}
                  >
                    <DownloadItem download={item} />
                  </motion.div>
                );
              })}
            </AnimatePresence>
          </div>
        )}
      </div>

      <DownloadSettingSheet
        setUrl={setUrl}
        url={url}
        open={isModalOpen}
        onOpenChange={setIsModalOpen}
      />
    </div>
  );
}
