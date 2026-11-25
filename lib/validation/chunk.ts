import { z } from 'zod';

export const chunkSchema = z.coerce
  .number()
  .min(1, { message: 'Chunk count must be at least 1.' })
  .max(16, { message: 'Chunk count cannot exceed 16.' });
