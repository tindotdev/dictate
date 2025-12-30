import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import { ConfigSchema, TranscriptionModelSchema } from '../config.js';

describe('ConfigSchema', () => {
  const originalEnv = process.env;

  beforeEach(() => {
    process.env = { ...originalEnv };
  });

  afterEach(() => {
    process.env = originalEnv;
  });

  it('validates complete config', () => {
    const result = ConfigSchema.safeParse({
      apiKey: 'sk-test-key',
      model: 'gpt-4o-mini-transcribe',
      prompt: 'technical terms',
      vadThreshold: 0.6,
      vadPrefixPaddingMs: 400,
      vadSilenceDurationMs: 600,
      noiseReduction: 'near_field',
      includeLogprobs: false,
      debug: true,
    });
    expect(result.success).toBe(true);
  });

  it('applies defaults for optional fields', () => {
    const result = ConfigSchema.safeParse({
      apiKey: 'sk-test-key',
    });
    expect(result.success).toBe(true);
    if (result.success) {
      expect(result.data.model).toBe('gpt-4o-mini-transcribe');
      expect(result.data.vadThreshold).toBe(0.5);
      expect(result.data.vadPrefixPaddingMs).toBe(300);
      expect(result.data.vadSilenceDurationMs).toBe(500);
      expect(result.data.noiseReduction).toBe('near_field');
      expect(result.data.includeLogprobs).toBe(false);
      expect(result.data.debug).toBe(false);
    }
  });

  it('requires apiKey', () => {
    const result = ConfigSchema.safeParse({});
    expect(result.success).toBe(false);
    if (!result.success) {
      const apiKeyError = result.error.issues.find(
        (issue) => issue.path[0] === 'apiKey'
      );
      expect(apiKeyError).toBeDefined();
    }
  });

  it('rejects empty apiKey', () => {
    const result = ConfigSchema.safeParse({ apiKey: '' });
    expect(result.success).toBe(false);
  });

  it('validates vadThreshold range', () => {
    // Valid: 0 to 1
    expect(ConfigSchema.safeParse({ apiKey: 'key', vadThreshold: 0 }).success).toBe(true);
    expect(ConfigSchema.safeParse({ apiKey: 'key', vadThreshold: 0.5 }).success).toBe(true);
    expect(ConfigSchema.safeParse({ apiKey: 'key', vadThreshold: 1 }).success).toBe(true);

    // Invalid: outside range
    expect(ConfigSchema.safeParse({ apiKey: 'key', vadThreshold: -0.1 }).success).toBe(false);
    expect(ConfigSchema.safeParse({ apiKey: 'key', vadThreshold: 1.1 }).success).toBe(false);
  });

  it('validates positive values for timing fields', () => {
    expect(
      ConfigSchema.safeParse({ apiKey: 'key', vadPrefixPaddingMs: 0 }).success
    ).toBe(false);
    expect(
      ConfigSchema.safeParse({ apiKey: 'key', vadSilenceDurationMs: -100 }).success
    ).toBe(false);
  });

  it('allows null noiseReduction', () => {
    const result = ConfigSchema.safeParse({
      apiKey: 'sk-test',
      noiseReduction: null,
    });
    expect(result.success).toBe(true);
    if (result.success) {
      expect(result.data.noiseReduction).toBeNull();
    }
  });
});

describe('TranscriptionModelSchema', () => {
  it('accepts valid models', () => {
    expect(TranscriptionModelSchema.safeParse('gpt-4o-transcribe').success).toBe(true);
    expect(TranscriptionModelSchema.safeParse('gpt-4o-mini-transcribe').success).toBe(true);
  });

  it('rejects invalid models', () => {
    expect(TranscriptionModelSchema.safeParse('whisper-1').success).toBe(false);
    expect(TranscriptionModelSchema.safeParse('gpt-4').success).toBe(false);
  });
});
