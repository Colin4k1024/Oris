import * as ed from "@noble/ed25519";

export async function signBody(seed: Uint8Array, body: Uint8Array): Promise<string> {
  const privateKey = seed;
  const signature = await ed.signAsync(body, privateKey);
  return toBase64(signature);
}

export async function signPayload(seed: Uint8Array, payload: unknown): Promise<string> {
  const data = new TextEncoder().encode(JSON.stringify(payload));
  return signBody(seed, data);
}

export async function publicKeyBase64(seed: Uint8Array): Promise<string> {
  const pub = await ed.getPublicKeyAsync(seed);
  return toBase64(pub);
}

export async function publicKeyHex(seed: Uint8Array): Promise<string> {
  const pub = await ed.getPublicKeyAsync(seed);
  return toHex(pub);
}

function toBase64(bytes: Uint8Array): string {
  if (typeof Buffer !== "undefined") {
    return Buffer.from(bytes).toString("base64");
  }
  let binary = "";
  for (const b of bytes) binary += String.fromCharCode(b);
  return btoa(binary);
}

function toHex(bytes: Uint8Array): string {
  return Array.from(bytes)
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
}
