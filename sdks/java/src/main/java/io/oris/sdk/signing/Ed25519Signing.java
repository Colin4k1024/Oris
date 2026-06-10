package io.oris.sdk.signing;

import com.fasterxml.jackson.databind.ObjectMapper;
import org.bouncycastle.crypto.params.Ed25519PrivateKeyParameters;
import org.bouncycastle.crypto.params.Ed25519PublicKeyParameters;
import org.bouncycastle.crypto.signers.Ed25519Signer;

import java.nio.charset.StandardCharsets;
import java.util.Base64;

public final class Ed25519Signing {

    private static final ObjectMapper MAPPER = new ObjectMapper();

    private Ed25519Signing() {}

    public static String signBody(byte[] seed, byte[] body) {
        Ed25519PrivateKeyParameters privateKey = new Ed25519PrivateKeyParameters(seed, 0);
        Ed25519Signer signer = new Ed25519Signer();
        signer.init(true, privateKey);
        signer.update(body, 0, body.length);
        byte[] signature = signer.generateSignature();
        return Base64.getEncoder().encodeToString(signature);
    }

    public static String signPayload(byte[] seed, Object payload) {
        try {
            byte[] data = MAPPER.writeValueAsBytes(payload);
            return signBody(seed, data);
        } catch (Exception e) {
            throw new RuntimeException("failed to sign payload", e);
        }
    }

    public static String publicKeyBase64(byte[] seed) {
        Ed25519PrivateKeyParameters privateKey = new Ed25519PrivateKeyParameters(seed, 0);
        Ed25519PublicKeyParameters publicKey = privateKey.generatePublicKey();
        return Base64.getEncoder().encodeToString(publicKey.getEncoded());
    }

    public static String publicKeyHex(byte[] seed) {
        Ed25519PrivateKeyParameters privateKey = new Ed25519PrivateKeyParameters(seed, 0);
        Ed25519PublicKeyParameters publicKey = privateKey.generatePublicKey();
        byte[] encoded = publicKey.getEncoded();
        StringBuilder sb = new StringBuilder(encoded.length * 2);
        for (byte b : encoded) {
            sb.append(String.format("%02x", b));
        }
        return sb.toString();
    }
}
