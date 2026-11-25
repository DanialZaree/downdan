import { z } from 'zod';

import { authSchema } from './auth';
import { chunkSchema } from './chunk';
import { cookiesArraySchema } from './cookies';
import { filePathSchema } from './file-path';
import { headersArraySchema } from './headers';
import { positiveNumberSchema } from './positive-number';
import { proxySchema } from './proxy';
import { urlSchema } from './url';

export const downloadFormSchema = z.object({
  proxy: proxySchema,
  auth: authSchema,
  url: urlSchema,
  engine: z.enum(['native', 'aria2']).optional(),
  chunk: chunkSchema,
  filePath: filePathSchema,
  headers: headersArraySchema,
  cookies: cookiesArraySchema,
  scheduleAt: z.string().optional(),
  speedLimit: positiveNumberSchema({
    message: 'Speed limit must be a positive number',
  }),
  maxRetries: positiveNumberSchema({
    message: 'Max retries must be a positive number',
  }),
  backoffFactor: positiveNumberSchema({
    message: 'Backoff factor must be a positive number',
  }),
  timeoutSecs: positiveNumberSchema({
    message: 'Timeout duration must be a positive number',
  }),
});
