import { describe, expect, it } from "vitest";
import { AUDIO_CONSTANTS } from "../pipewire.js";

describe("AUDIO_CONSTANTS", () => {
	it("has correct sample rate for OpenAI Realtime", () => {
		expect(AUDIO_CONSTANTS.SAMPLE_RATE).toBe(24000);
	});

	it("has correct channels (mono)", () => {
		expect(AUDIO_CONSTANTS.CHANNELS).toBe(1);
	});

	it("has correct bytes per sample (s16)", () => {
		expect(AUDIO_CONSTANTS.BYTES_PER_SAMPLE).toBe(2);
	});

	it("has 20ms frame duration", () => {
		expect(AUDIO_CONSTANTS.FRAME_MS).toBe(20);
	});

	it("calculates correct frame size in bytes", () => {
		// 24000 samples/sec * 2 bytes/sample * 1 channel * 0.020 sec = 960 bytes
		const expectedBytes =
			(AUDIO_CONSTANTS.SAMPLE_RATE *
				AUDIO_CONSTANTS.BYTES_PER_SAMPLE *
				AUDIO_CONSTANTS.CHANNELS *
				AUDIO_CONSTANTS.FRAME_MS) /
			1000;
		expect(AUDIO_CONSTANTS.FRAME_BYTES).toBe(expectedBytes);
		expect(AUDIO_CONSTANTS.FRAME_BYTES).toBe(960);
	});
});

describe("Audio chunking logic", () => {
	it("chunks buffer into correct frame sizes", () => {
		const FRAME_BYTES = AUDIO_CONSTANTS.FRAME_BYTES;

		// Simulate chunking logic from AudioCapture
		function chunkBuffer(input: Buffer): Buffer[] {
			const chunks: Buffer[] = [];
			let buffer = input;

			while (buffer.length >= FRAME_BYTES) {
				chunks.push(buffer.subarray(0, FRAME_BYTES));
				buffer = buffer.subarray(FRAME_BYTES);
			}

			return chunks;
		}

		// Test exact multiple of frame size
		const exactBuffer = Buffer.alloc(FRAME_BYTES * 3);
		const exactChunks = chunkBuffer(exactBuffer);
		expect(exactChunks).toHaveLength(3);
		expect(exactChunks[0].length).toBe(FRAME_BYTES);

		// Test with remainder
		const remainderBuffer = Buffer.alloc(FRAME_BYTES * 2 + 100);
		const remainderChunks = chunkBuffer(remainderBuffer);
		expect(remainderChunks).toHaveLength(2);

		// Test smaller than frame
		const smallBuffer = Buffer.alloc(100);
		const smallChunks = chunkBuffer(smallBuffer);
		expect(smallChunks).toHaveLength(0);

		// Test empty buffer
		const emptyBuffer = Buffer.alloc(0);
		const emptyChunks = chunkBuffer(emptyBuffer);
		expect(emptyChunks).toHaveLength(0);
	});

	it("base64 encodes audio correctly", () => {
		const testData = Buffer.from([0x00, 0x01, 0x02, 0x03]);
		const base64 = testData.toString("base64");
		expect(base64).toBe("AAECAw==");

		// Verify round-trip
		const decoded = Buffer.from(base64, "base64");
		expect(decoded).toEqual(testData);
	});
});
