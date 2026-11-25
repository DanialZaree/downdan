'use client';

import { TabsContent } from '@radix-ui/react-tabs';
import { useFormContext } from 'react-hook-form';

import { FormControl, FormDescription, FormField, FormItem, FormLabel } from '../ui/form';
import { Input } from '../ui/input';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '../ui/select';
import { Slider } from '../ui/slider';

import FormMessage from './form-message';

import type React from 'react';

interface BasicTabProps {
  handleKeyPress: (e: React.KeyboardEvent) => void;
}

export default function BasicTab({ handleKeyPress }: BasicTabProps) {
  const { control } = useFormContext();
  return (
    <TabsContent value="basic" className="space-y-4 p-2">
      <FormField
        control={control}
        name="url"
        render={({ field }) => (
          <FormItem className="flex-col gap-1">
            <FormLabel htmlFor="url">Download URL *</FormLabel>
            <FormControl>
              <Input
                {...field}
                id="url"
                inputMode="url"
                placeholder="https://example.com/file.mp4, .m3u8, or magnet:?"
                onKeyDown={handleKeyPress}
              />
            </FormControl>
            <FormDescription>
              Supports HTTP/HTTPS, HLS (.m3u8), plus FTP/SFTP/Magnet via aria2.
            </FormDescription>
            <FormMessage />
          </FormItem>
        )}
      />

      <FormField
        control={control}
        name="engine"
        render={({ field }) => (
          <FormItem className="flex-col gap-1">
            <FormLabel htmlFor="engine">Download Engine</FormLabel>
            <Select value={field.value ?? 'aria2'} onValueChange={field.onChange}>
              <SelectTrigger id="engine">
                <SelectValue placeholder="Select engine" />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="aria2">aria2 (Default — HTTP/FTP/SFTP/Magnet)</SelectItem>
                <SelectItem value="native">Native (Rust multi-chunk + BLAKE3)</SelectItem>
              </SelectContent>
            </Select>
            <FormMessage />
          </FormItem>
        )}
      />

      <FormField
        control={control}
        name="chunk"
        render={({ field }) => {
          const val = typeof field.value === 'number' ? field.value : 8;
          return (
            <FormItem className="flex-col gap-1">
              <div className="flex items-center justify-between">
                <FormLabel htmlFor="chunk">Parallel chunks</FormLabel>
                <span className="text-muted-foreground text-xs">{val}</span>
              </div>
              <FormControl>
                <Slider
                  min={1}
                  max={16}
                  step={1}
                  value={[val]}
                  onValueChange={(v) => field.onChange(v[0])}
                />
              </FormControl>
              <FormDescription>
                Parallel connections with dynamic work-stealing (1–16).
              </FormDescription>
              <FormMessage />
            </FormItem>
          );
        }}
      />

      <FormField
        control={control}
        name="scheduleAt"
        render={({ field }) => (
          <FormItem className="flex-col gap-1">
            <FormLabel htmlFor="scheduleAt">Schedule start (optional)</FormLabel>
            <FormControl>
              <Input
                id="scheduleAt"
                type="datetime-local"
                value={field.value ?? ''}
                onChange={(e) => field.onChange(e.target.value)}
              />
            </FormControl>
            <FormDescription>
              Leave blank to start immediately, or pick a future time.
            </FormDescription>
            <FormMessage />
          </FormItem>
        )}
      />
    </TabsContent>
  );
}
