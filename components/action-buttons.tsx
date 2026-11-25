'use client';

import { invoke } from '@tauri-apps/api/core';
import { revealItemInDir } from '@tauri-apps/plugin-opener';
import { Folder, Link2, Pause, Play, RotateCw, X } from 'lucide-react';
import { useCallback, useState } from 'react';

import { RemoveDownloadDialog } from './confirm-modal';
import { useDownloads } from './download-context';
import { RefreshUrlDialog } from './refresh-url-dialog';
import { Status } from './types';
import { Button } from './ui/button';

const buttonClassName = 'h-9 w-9 font-medium transition-all duration-200';

interface ActionButtonsProps {
  status: Status;
  downloadId: number;
  url: string;
  filePath: string;
  filename: string;
  fileExist: boolean;
}

export default function ActionButtons({
  status,
  downloadId,
  url,
  filePath,
  fileExist,
  filename,
}: ActionButtonsProps) {
  const [confirmOpen, setConfirmOpen] = useState(false);
  const [refreshOpen, setRefreshOpen] = useState(false);
  const { removeDownload } = useDownloads();

  const isResumeDisabled = status === Status.Writing;
  const canToggle = status !== Status.Completed && status !== Status.Failed;
  const canRemove = status !== Status.Downloading && status !== Status.Writing;
  const canReveal = status === Status.Completed && fileExist;
  const canRetry = status === Status.Failed;
  const canRefreshUrl = status === Status.Paused || status === Status.Failed;

  const handleToggleDownload = useCallback(async () => {
    if (status === Status.Paused || status === Status.Failed) {
      await invoke('resume_download', { id: downloadId });
    } else {
      await invoke('pause_download', { id: downloadId });
    }
  }, [status, downloadId]);

  const handleRemoveConfirmed = useCallback(
    async (removeFile?: boolean) => {
      await removeDownload(downloadId, !!removeFile);
      setConfirmOpen(false);
    },
    [removeDownload, downloadId],
  );

  const handleOpenFile = useCallback(async () => {
    try {
      await revealItemInDir(filePath);
    } catch (e) {
      console.error('Failed to reveal in folder:', e);
    }
  }, [filePath]);

  return (
    <>
      <RemoveDownloadDialog
        open={confirmOpen}
        onOpenChange={setConfirmOpen}
        onConfirm={handleRemoveConfirmed}
        filename={filename}
        fileExist={fileExist}
      />

      <RefreshUrlDialog
        open={refreshOpen}
        onOpenChange={setRefreshOpen}
        downloadId={downloadId}
        currentUrl={url}
        filename={filename}
      />

      <div className="flex gap-2">
        {canToggle && (
          <Button
            onClick={handleToggleDownload}
            variant="outline"
            size="sm"
            className={buttonClassName}
            disabled={isResumeDisabled}
            aria-label={status === Status.Paused ? 'Resume' : 'Pause'}
            title={status === Status.Paused ? 'Resume' : 'Pause'}
          >
            {status === Status.Paused ? (
              <Play className="h-4 w-4" />
            ) : (
              <Pause className="h-4 w-4" />
            )}
          </Button>
        )}

        {canRetry && (
          <Button
            onClick={handleToggleDownload}
            variant="outline"
            size="sm"
            className={buttonClassName}
            aria-label="Retry"
            title="Retry"
          >
            <RotateCw className="h-4 w-4" />
          </Button>
        )}

        {canRefreshUrl && (
          <Button
            onClick={() => setRefreshOpen(true)}
            variant="outline"
            size="sm"
            className={buttonClassName}
            aria-label="Refresh expired URL"
            title="Refresh expired URL"
          >
            <Link2 className="h-4 w-4" />
          </Button>
        )}

        {canRemove && (
          <Button
            onClick={() => setConfirmOpen(true)}
            variant="outline"
            size="sm"
            className={buttonClassName}
            aria-label="Remove"
            title="Remove"
          >
            <X className="h-4 w-4" />
          </Button>
        )}

        {canReveal && (
          <Button
            onClick={handleOpenFile}
            variant="outline"
            size="sm"
            className={buttonClassName}
            aria-label="Show in folder"
            title="Show in folder"
          >
            <Folder className="h-4 w-4" />
          </Button>
        )}
      </div>
    </>
  );
}
