import { z } from 'zod';

export const urlSchema = z.string().refine(
  (val) => /^(https?|s?ftp):\/\/\S+$/i.test(val) || /^magnet:\?\S+$/i.test(val),
  {
    message: 'URL is not valid.',
  },
);

export const urlFormSchema = z.object({ url: urlSchema });
