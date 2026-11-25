import { useState } from 'react';

import { FormControl, FormField, FormItem, FormLabel } from '../ui/form';
import { Input } from '../ui/input';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '../ui/select';

import FormMessage from './form-message';

import type { DownloadFormData } from './download-setting-sheet';
import type React from 'react';
import type { UseFormReturn } from 'react-hook-form';

const UNIT_MULTIPLIERS = {
  KB: 1024,
  MB: 1024 * 1024,
  GB: 1024 * 1024 * 1024,
} as const;

type Unit = keyof typeof UNIT_MULTIPLIERS;

interface SpeedLimitFieldProps {
  form: UseFormReturn<DownloadFormData>;
}

export default function SpeedLimitField({ form }: SpeedLimitFieldProps) {
  const [unit, setUnit] = useState<Unit>('MB');
  const byteValue = form.watch('speedLimit');

  const displayValue =
    typeof byteValue === 'number' && byteValue > 0
      ? String(Math.max(1, Math.round(byteValue / UNIT_MULTIPLIERS[unit])))
      : '';

  const handleChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const raw = e.target.value.trim();
    if (!raw) {
      form.setValue('speedLimit', undefined, { shouldValidate: true });
      return;
    }
    const numericValue = Number.parseFloat(raw);
    if (Number.isFinite(numericValue) && numericValue > 0) {
      form.setValue('speedLimit', Math.floor(numericValue * UNIT_MULTIPLIERS[unit]), {
        shouldValidate: true,
      });
    } else {
      form.setValue('speedLimit', undefined, { shouldValidate: true });
    }
  };

  const handleUnitChange = (nextUnit: Unit) => {
    const currentDisplay =
      typeof byteValue === 'number' && byteValue > 0
        ? Math.max(1, Math.round(byteValue / UNIT_MULTIPLIERS[unit]))
        : undefined;
    setUnit(nextUnit);
    if (currentDisplay !== undefined) {
      form.setValue('speedLimit', Math.floor(currentDisplay * UNIT_MULTIPLIERS[nextUnit]), {
        shouldValidate: true,
      });
    }
  };

  return (
    <FormField
      control={form.control}
      name="speedLimit"
      render={() => (
        <FormItem className="flex-col gap-1">
          <FormLabel htmlFor="speedLimit">Speed Limit (per download)</FormLabel>
          <div className="flex items-center gap-2">
            <FormControl>
              <Input
                id="speedLimit"
                type="number"
                placeholder="Unlimited"
                value={displayValue}
                onChange={handleChange}
                min={1}
              />
            </FormControl>
            <Select value={unit} onValueChange={(value) => handleUnitChange(value as Unit)}>
              <SelectTrigger className="w-[100px]">
                <SelectValue placeholder="Unit" />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="KB">KB/s</SelectItem>
                <SelectItem value="MB">MB/s</SelectItem>
                <SelectItem value="GB">GB/s</SelectItem>
              </SelectContent>
            </Select>
          </div>
          <FormMessage />
        </FormItem>
      )}
    />
  );
}
