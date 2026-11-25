'use client';

import { invoke } from '@tauri-apps/api/core';
import { Link2 } from 'lucide-react';
import { useEffect, useState } from 'react';
import { toast } from 'sonner';

import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Loading } from '@/components/ui/loading';

interface RefreshUrlDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  downloadId: number;
  currentUrl: string;
  filename: string;
}

export function RefreshUrlDialog({
  open,
  onOpenChange,
  downloadId,
  currentUrl,
  filename,
}: RefreshUrlDialogProps) {
  const [newUrl, setNewUrl] = useState(currentUrl);
  const [autoResume, setAutoResume] = useState(true);
  const [isSaving, setIsSaving] = useState(false);

  useEffect(() => {
    if (open) {
      setNewUrl(currentUrl);
    }
  }, [open, currentUrl]);

  const handleSave = async () => {
    const trimmed = newUrl.trim();
    if (!trimmed) {
      toast.error('Please enter a valid URL');
      return;
    }

    setIsSaving(true);
    try {
      await invoke('update_download_url', {
        id: downloadId,
        url: trimmed,
        resume: autoResume,
      });
      toast.success(
        autoResume ? 'Download URL refreshed & resumed' : 'Download URL updated',
      );
      onOpenChange(false);
    } catch (err) {
      toast.error('Failed to update URL', { description: String(err) });
    } finally {
      setIsSaving(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={(next) => !isSaving && onOpenChange(next)}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Link2 className="h-4 w-4" />
            Refresh Download Link
          </DialogTitle>
          <DialogDescription>
            Paste a fresh signed link for <b>{filename}</b>. Existing BLAKE3-verified chunks on
            disk are preserved.
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-3">
          <div className="space-y-1.5">
            <Label htmlFor="refresh-url-input">New URL</Label>
            <Input
              id="refresh-url-input"
              value={newUrl}
              onChange={(e) => setNewUrl(e.target.value)}
              placeholder="https://cdn.example.com/file.zip?token=..."
            />
          </div>

          <div className="flex items-center space-x-2">
            <Checkbox
              id="refresh-auto-resume"
              checked={autoResume}
              onCheckedChange={(checked) => setAutoResume(!!checked)}
            />
            <label htmlFor="refresh-auto-resume" className="text-xs">
              Resume download immediately after updating
            </label>
          </div>
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)} disabled={isSaving}>
            Cancel
          </Button>
          <Button onClick={handleSave} disabled={isSaving}>
            {isSaving ? <Loading className="h-4 w-4" /> : 'Save & Continue'}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
