'use client';

import { zodResolver } from '@hookform/resolvers/zod';
import { invoke } from '@tauri-apps/api/core';
import { useEffect, useState } from 'react';
import { useForm } from 'react-hook-form';
import { toast } from 'sonner';

import { Button } from '@/components/ui/button';
import { Form } from '@/components/ui/form';
import { getAppSettings } from '@/lib/app-settings';
import { downloadFormSchema } from '@/lib/validation';

import { Loading } from '../ui/loading';
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from '../ui/sheet';
import { Tabs, TabsList, TabsTrigger } from '../ui/tabs';

import AdvancedTab from './advanced-tab';
import BasicTab from './basic-tab';

import type React from 'react';
import type { z } from 'zod';

export type DownloadFormData = z.infer<typeof downloadFormSchema>;

interface DownloadSettingSheetProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  url: string;
  setUrl: (url: string) => void;
}

const getDefaultFormValues = (url: string): DownloadFormData => {
  const defaults = getAppSettings();
  return {
    maxRetries: defaults.maxRetries,
    backoffFactor: defaults.backoffFactor,
    timeoutSecs: defaults.timeoutSecs,
    url,
    engine: defaults.defaultEngine,
    chunk: defaults.chunkCount,
    scheduleAt: '',
    headers: [],
    cookies: [],
    auth: { type: 'None' },
    proxy: { type: 'system' },
    filePath: defaults.defaultDownloadDir || '',
  };
};

export default function DownloadSettingSheet({
  open,
  onOpenChange,
  setUrl,
  url,
}: DownloadSettingSheetProps) {
  const [isLoading, setIsLoading] = useState(false);
  const form = useForm<DownloadFormData>({
    resolver: zodResolver(downloadFormSchema),
    defaultValues: getDefaultFormValues(url),
    mode: 'onBlur',
  });

  useEffect(() => {
    if (open) {
      const defaults = getDefaultFormValues(url.trim());
      form.reset(defaults);
    }
  }, [form, open, url]);

  const handleSubmit = async (values: DownloadFormData) => {
    if (!values.url?.trim()) {
      form.setError('url', { type: 'manual', message: 'URL is required' });
      return;
    }

    let startAtUnixMs: number | undefined;
    if (values.scheduleAt?.trim()) {
      const parsedMs = new Date(values.scheduleAt).getTime();
      if (Number.isNaN(parsedMs) || parsedMs <= Date.now()) {
        form.setError('scheduleAt', {
          type: 'manual',
          message: 'Please choose a future date and time',
        });
        return;
      }
      startAtUnixMs = parsedMs;
    }

    const kvToRecord = (arr?: Array<{ key: string; value: string }>) =>
      (arr || []).reduce<Record<string, string>>((acc, { key, value }) => {
        if (key && value) {
          acc[key] = value;
        }
        return acc;
      }, {});

    const proxy =
      values.proxy?.type !== 'none' && values.proxy?.type !== 'system'
        ? {
            type: values.proxy.type.toLowerCase(),
            host: values.proxy.host,
            port: values.proxy.port,
            username: values.proxy?.auth?.username,
            password: values.proxy?.auth?.password,
          }
        : undefined;
    const headers = kvToRecord(values.headers);
    const cookies = kvToRecord(values.cookies);

    const defaults = getAppSettings();
    setIsLoading(true);
    try {
      await invoke('add_new_download', {
        url: values.url.trim(),
        options: {
          proxy,
          ...(values.auth &&
            values.auth?.type !== 'None' && {
              auth: { ...values.auth, type: values.auth.type.toLowerCase() },
            }),
          headers: Object.keys(headers).length ? headers : undefined,
          cookies: Object.keys(cookies).length ? cookies : undefined,
          chunk_count: values.chunk,
          ...(!!values.filePath && {
            file_path: values.filePath,
          }),
          speed_limit: values.speedLimit || undefined,
          max_retries: values.maxRetries || undefined,
          backoff_factor: values.backoffFactor || undefined,
          timeout_secs: values.timeoutSecs || undefined,
          start_at_unix_ms: startAtUnixMs,
          engine: values.engine ?? defaults.defaultEngine,
          aria2_path: defaults.aria2Path || undefined,
        },
      });

      toast.success(startAtUnixMs ? 'Download scheduled' : 'Download added');
      onOpenChange(false);
      setUrl('');
      form.reset(getDefaultFormValues(''));
    } catch (err: unknown) {
      const error = err as Error;
      console.error('Failed to add download:', err);
      toast.error('Failed to add download', {
        description:
          typeof err === 'string'
            ? err
            : typeof error?.message === 'string'
              ? error.message
              : 'Please check the URL or settings and try again.',
      });
    } finally {
      setIsLoading(false);
    }
  };

  const onKeyPress = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      form.handleSubmit(handleSubmit)();
    }
  };

  return (
    <Sheet open={open} onOpenChange={(v) => !isLoading && onOpenChange(v)}>
      <SheetContent className="h-full p-2 sm:max-w-md">
        <SheetHeader className="px-1">
          <SheetTitle>New download</SheetTitle>
          <SheetDescription>Paste a URL, tweak options, done.</SheetDescription>
        </SheetHeader>

        <Form {...form}>
          <form
            onSubmit={form.handleSubmit(handleSubmit)}
            className="flex h-full flex-col gap-2 overflow-y-auto"
          >
            <Tabs defaultValue="basic" className="flex-1">
              <TabsList className="sticky top-0 z-10 grid w-full grid-cols-2 bg-neutral-200 dark:bg-neutral-900">
                <TabsTrigger value="basic">Basic</TabsTrigger>
                <TabsTrigger value="advanced">Advanced</TabsTrigger>
              </TabsList>

              <BasicTab handleKeyPress={onKeyPress} />
              <AdvancedTab handleKeyPress={onKeyPress} />
            </Tabs>

            <div className="bg-background sticky bottom-0 flex justify-end gap-2 border-t p-2">
              <Button
                type="button"
                variant="outline"
                onClick={() => onOpenChange(false)}
                disabled={isLoading}
              >
                Cancel
              </Button>
              <Button type="submit" disabled={isLoading}>
                {isLoading ? <Loading className="h-4 w-4" /> : 'Start download'}
              </Button>
            </div>
          </form>
        </Form>
      </SheetContent>
    </Sheet>
  );
}
