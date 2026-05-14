import { describe, it, expect } from "vitest";
import { signBody, signPayload, publicKeyBase64, publicKeyHex } from "../src/signing.js";

const SEED = new TextEncoder().encode("test-seed-32-bytes-for-testing!!");

describe("signing", () => {
  it("signBody returns base64 of 64-byte signature", async () => {
    const sig = await signBody(SEED, new TextEncoder().encode('{"hello":"world"}'));
    const raw = Buffer.from(sig, "base64");
    expect(raw.length).toBe(64);
  });

  it("signPayload is deterministic", async () => {
    const payload = { gene_id: "g1", confidence: 0.9 };
    const sig1 = await signPayload(SEED, payload);
    const sig2 = await signPayload(SEED, payload);
    expect(sig1).toBe(sig2);
    expect(Buffer.from(sig1, "base64").length).toBe(64);
  });

  it("publicKeyBase64 returns 32 bytes encoded", async () => {
    const pk = await publicKeyBase64(SEED);
    const raw = Buffer.from(pk, "base64");
    expect(raw.length).toBe(32);
  });

  it("publicKeyHex returns 64 hex chars", async () => {
    const pk = await publicKeyHex(SEED);
    expect(pk.length).toBe(64);
    expect(/^[0-9a-f]+$/.test(pk)).toBe(true);
  });
});
