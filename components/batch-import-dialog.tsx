'use client';

import { invoke } from '@tauri-apps/api/core';
import { ListPlus } from 'lucide-react';
import { useMemo, useState } from 'react';
import { toast } from 'sonner';

import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Label } from '@/components/ui/label';
import { Loading } from '@/components/ui/loading';
import { extractUrlsFromText, getAppSettings } from '@/lib/app-settings';

interface BatchResult {
  added: number;
  failed: string[];
}

interface BatchImportDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

export function BatchImportDialog({ open, onOpenChange }: BatchImportDialogProps) {
  const [rawText, setRawText] = useState('');
  const [isSubmitting, setIsSubmitting] = useState(false);

  const detectedUrls = useMemo(() => extractUrlsFromText(rawText), [rawText]);

  const handleImport = async () => {
    if (detectedUrls.length === 0) {
      toast.error('No valid HTTP/HTTPS URLs found');
      return;
    }

    const defaults = getAppSettings();
    setIsSubmitting(true);
    try {
      const result = await invoke<BatchResult>('add_batch_downloads', {
        urls: detectedUrls,
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

      if (result.added > 0) {
        toast.success(`Queued ${result.added} download${result.added === 1 ? '' : 's'}`);
      }
      if (result.failed.length > 0) {
        toast.error(`${result.failed.length} URL(s) failed`, {
          description: result.failed[0],
        });
      }
      setRawText('');
      onOpenChange(false);
    } catch (err) {
      toast.error('Batch import failed', { description: String(err) });
    } finally {
      setIsSubmitting(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={(next) => !isSubmitting && onOpenChange(next)}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <ListPlus className="h-5 w-5" />
            Batch URL Import
          </DialogTitle>
          <DialogDescription>
            Paste multiple URLs (one per line) or any text containing links. danDown will
            automatically extract and deduplicate valid HTTP/HTTPS links.
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-2">
          <div className="flex items-center justify-between">
            <Label htmlFor="batch-urls">Links</Label>
            <span className="text-muted-foreground text-xs">
              {detectedUrls.length} valid URL{detectedUrls.length === 1 ? '' : 's'} detected
            </span>
          </div>
          <textarea
            id="batch-urls"
            rows={7}
            value={rawText}
            onChange={(e) => setRawText(e.target.value)}
            placeholder={'https://example.com/part1.zip\nhttps://example.com/part2.zip'}
            className="border-input bg-background ring-offset-background placeholder:text-muted-foreground focus-visible:ring-ring w-full rounded-md border px-3 py-2 text-xs focus-visible:ring-2 focus-visible:outline-none"
          />
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)} disabled={isSubmitting}>
            Cancel
          </Button>
          <Button onClick={handleImport} disabled={isSubmitting || detectedUrls.length === 0}>
            {isSubmitting ? <Loading className="h-4 w-4" /> : `Queue ${detectedUrls.length || ''}`}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
