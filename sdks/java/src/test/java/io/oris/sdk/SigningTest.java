package io.oris.sdk;

import io.oris.sdk.signing.Ed25519Signing;
import org.junit.jupiter.api.Test;

import java.util.Map;

import static org.junit.jupiter.api.Assertions.*;

class SigningTest {

    private static final byte[] TEST_SEED = new byte[32];

    static {
        for (int i = 0; i < 32; i++) TEST_SEED[i] = (byte) (i + 1);
    }

    @Test
    void signBodyProducesBase64Signature() {
        byte[] body = "hello world".getBytes();
        String sig = Ed25519Signing.signBody(TEST_SEED, body);
        assertNotNull(sig);
        assertFalse(sig.isEmpty());
        // Ed25519 signature is 64 bytes = 88 chars base64
        assertEquals(88, sig.length());
    }

    @Test
    void signPayloadSerializesAndSigns() {
        Map<String, Object> payload = Map.of("gene_id", "g1", "confidence", 0.9);
        String sig = Ed25519Signing.signPayload(TEST_SEED, payload);
        assertNotNull(sig);
        assertEquals(88, sig.length());
    }

    @Test
    void publicKeyBase64Returns44Chars() {
        String pk = Ed25519Signing.publicKeyBase64(TEST_SEED);
        assertNotNull(pk);
        assertEquals(44, pk.length());
    }

    @Test
    void publicKeyHexReturns64Chars() {
        String pk = Ed25519Signing.publicKeyHex(TEST_SEED);
        assertNotNull(pk);
        assertEquals(64, pk.length());
        assertTrue(pk.matches("[0-9a-f]+"));
    }

    @Test
    void deterministicSignatures() {
        byte[] body = "deterministic test".getBytes();
        String sig1 = Ed25519Signing.signBody(TEST_SEED, body);
        String sig2 = Ed25519Signing.signBody(TEST_SEED, body);
        assertEquals(sig1, sig2);
    }
}
