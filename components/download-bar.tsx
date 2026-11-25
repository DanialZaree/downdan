'use client';

import { zodResolver } from '@hookform/resolvers/zod';
import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { DownloadIcon, FileUp, ListPlus, MoreHorizontal } from 'lucide-react';
import { useEffect, useRef, useState } from 'react';
import { useForm } from 'react-hook-form';
import { toast } from 'sonner';

import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { getAppSettings, looksLikeDownloadableUrl } from '@/lib/app-settings';
import { cn } from '@/lib/utils';
import { urlFormSchema } from '@/lib/validation';

import { BatchImportDialog } from './batch-import-dialog';
import { Form, FormControl, FormField, FormItem } from './ui/form';
import { Loading } from './ui/loading';

import type React from 'react';
import type { z } from 'zod';

interface DownloadBarProps {
  setIsModalOpen: (open: boolean) => void;
  url: string;
  setUrl: (url: string) => void;
}

type UrlFormData = z.infer<typeof urlFormSchema>;

export default function DownloadBar({ setIsModalOpen, setUrl, url }: DownloadBarProps) {
  const [isLoading, setIsLoading] = useState(false);
  const [batchOpen, setBatchOpen] = useState(false);
  const lastCapturedClipboard = useRef<string>('');

  const form = useForm<UrlFormData>({
    resolver: zodResolver(urlFormSchema),
    defaultValues: { url },
    mode: 'onChange',
  });

  useEffect(() => {
    if (form.getValues('url') !== url) {
      form.setValue('url', url, { shouldValidate: false });
    }
  }, [form, url]);

  useEffect(() => {
    const checkClipboard = async () => {
      const settings = getAppSettings();
      if (!settings.clipboardMonitor || form.getValues('url').trim()) {
        return;
      }
      try {
        const clip = (await navigator.clipboard.readText())?.trim();
        if (!clip || clip === lastCapturedClipboard.current) {
          return;
        }
        if (looksLikeDownloadableUrl(clip)) {
          lastCapturedClipboard.current = clip;
          setUrl(clip);
          form.setValue('url', clip, { shouldValidate: true });
          toast.info('Download link detected from clipboard');
        }
      } catch {
        // Clipboard permission unavailable when unfocused; ignore quietly.
      }
    };

    window.addEventListener('focus', checkClipboard);
    return () => window.removeEventListener('focus', checkClipboard);
  }, [form, setUrl]);

  const urlError = form.formState.errors.url;

  const handleSubmit = async (value: z.infer<typeof urlFormSchema>) => {
    if (!value.url.trim()) {
      return;
    }

    const defaults = getAppSettings();
    setIsLoading(true);
    try {
      await invoke('add_new_download', {
        url: value.url.trim(),
        options: {
          chunk_count: defaults.chunkCount,
          max_retries: defaults.maxRetries,
          backoff_factor: defaults.backoffFactor,
          timeout_secs: defaults.timeoutSecs,
          engine: defaults.defaultEngine,
          aria2_path: defaults.aria2Path || undefined,
          ...(defaults.defaultDownloadDir ? { file_path: defaults.defaultDownloadDir } : {}),
        },
      });
      form.setValue('url', '');
      setUrl('');
    } catch (error) {
      toast.error(`${error}`);
    } finally {
      setIsLoading(false);
    }
  };

  const handleImportFile = async () => {
    try {
      const selected = await open({
        directory: false,
        multiple: true,
        title: 'Import .torrent or URL list (.txt)',
        filters: [
          { name: 'Torrents & URL Lists', extensions: ['torrent', 'txt', 'lst', 'urls'] },
        ],
      });
      const paths = Array.isArray(selected) ? selected : selected ? [selected] : [];
      if (paths.length === 0) {
        return;
      }
      const count = await invoke<number>('import_dropped_files', { paths });
      if (count > 0) {
        toast.success(`Queued ${count} download${count === 1 ? '' : 's'}`);
      } else {
        toast.info('No downloadable links found in selected file');
      }
    } catch (err) {
      toast.error('Failed to import file', { description: String(err) });
    }
  };

  const handleUrlFieldChange = (ev: React.ChangeEvent<HTMLInputElement>) => {
    if (urlError) {
      form.clearErrors('url');
    }
    setUrl(ev.target.value);
    form.setValue('url', ev.target.value, { shouldValidate: false });
  };

  return (
    <>
      <BatchImportDialog open={batchOpen} onOpenChange={setBatchOpen} />

      <div
        className={cn(
          'bg-card flex items-start justify-between gap-2 border-b border-b-transparent py-1 pr-3 pl-1',
          {
            'border-b border-b-red-600/30': urlError,
          },
        )}
      >
        <Form {...form}>
          <form
            className="flex w-full gap-2"
            onSubmit={form.handleSubmit(handleSubmit, (errors) => {
              if (errors.url?.message) {
                toast.error(errors.url.message);
              }
            })}
          >
            <FormField
              control={form.control}
              name="url"
              render={({ field }) => (
                <FormItem className="w-full gap-1">
                  <FormControl>
                    <Input
                      inputMode="url"
                      placeholder="Enter a link (.zip, .mp4, .m3u8, magnet:?…) to download."
                      {...field}
                      onChange={handleUrlFieldChange}
                      className={cn(
                        'w-full border-none !bg-inherit shadow-none focus-visible:ring-0',
                      )}
                    />
                  </FormControl>
                </FormItem>
              )}
            />
            <Button
              variant="ghost"
              type="submit"
              disabled={isLoading}
              className="flex items-center"
              title="Download now"
              aria-label="Download now"
            >
              {isLoading ? <Loading className="h-4 w-4" /> : <DownloadIcon className="h-4 w-4" />}
            </Button>
          </form>
        </Form>

        <Button
          type="button"
          onClick={handleImportFile}
          variant="ghost"
          title="Import .torrent or .txt URL list"
          aria-label="Import .torrent or .txt URL list"
        >
          <FileUp className="h-4 w-4" />
        </Button>

        <Button
          type="button"
          onClick={() => setBatchOpen(true)}
          variant="ghost"
          title="Batch URL import"
          aria-label="Batch URL import"
        >
          <ListPlus className="h-4 w-4" />
        </Button>

        <Button
          type="button"
          onClick={() => setIsModalOpen(true)}
          className="flex items-center gap-1"
          variant="ghost"
          title="Advanced download options"
          aria-label="Advanced download options"
        >
          <MoreHorizontal className="h-4 w-4" />
        </Button>
      </div>
    </>
  );
}
