import { z } from 'zod';

export const TranscriptionModelSchema = z.enum([
  'gpt-4o-transcribe',
  'gpt-4o-mini-transcribe',
]);

export type TranscriptionModel = z.infer<typeof TranscriptionModelSchema>;

export const NoiseReductionSchema = z.enum(['near_field', 'far_field']);

export type NoiseReduction = z.infer<typeof NoiseReductionSchema>;

// Default prompt biases transcription toward software engineering terminology
const DEFAULT_PROMPT =
  'TypeScript, JavaScript, Python, Node.js, Bun, React, Next.js, Cloudflare Workers, ' +
  'monorepo, API, REST, GraphQL, WebSocket, OAuth, JWT, CI/CD, Docker, Kubernetes, ' +
  'domain-driven design, microservices, serverless, async, await, function, interface.';

export const ConfigSchema = z.object({
  apiKey: z.string().min(1, 'OPENAI_API_KEY is required'),
  model: TranscriptionModelSchema.default('gpt-4o-transcribe'),
  // ISO-639-1 language code (e.g., 'en', 'es', 'fr'). Biases transcription toward this language.
  language: z.string().length(2).default('en'),
  // Prompt helps with spelling of technical terms and vocabulary
  prompt: z.string().default(DEFAULT_PROMPT),
  vadThreshold: z.number().min(0).max(1).default(0.5),
  vadPrefixPaddingMs: z.number().positive().default(300),
  vadSilenceDurationMs: z.number().positive().default(500),
  noiseReduction: NoiseReductionSchema.nullable().default('near_field'),
  includeLogprobs: z.boolean().default(false),
  debug: z.boolean().default(false),
});

export type Config = z.infer<typeof ConfigSchema>;

export function loadConfig(): Config {
  const rawConfig = {
    apiKey: process.env.OPENAI_API_KEY ?? '',
    model: process.env.OPENAI_STT_MODEL ?? 'gpt-4o-transcribe',
    language: process.env.OPENAI_STT_LANGUAGE ?? 'en',
    prompt: process.env.OPENAI_STT_PROMPT,
    debug: process.env.DEBUG === '1',
  };

  const result = ConfigSchema.safeParse(rawConfig);

  if (!result.success) {
    const errors = result.error.issues
      .map((issue) => `${issue.path.join('.')}: ${issue.message}`)
      .join(', ');
    throw new Error(`Configuration error: ${errors}`);
  }

  return result.data;
}
