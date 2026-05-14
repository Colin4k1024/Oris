import base64
import json

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey


def _private_key(seed: bytes) -> Ed25519PrivateKey:
    if len(seed) != 32:
        raise ValueError("seed must be 32 bytes")
    return Ed25519PrivateKey.from_private_bytes(seed)


def sign_body(seed: bytes, body: bytes) -> str:
    key = _private_key(seed)
    sig = key.sign(body)
    return base64.b64encode(sig).decode()


def sign_payload(seed: bytes, payload: object) -> str:
    data = json.dumps(payload, separators=(",", ":"), ensure_ascii=False).encode()
    return sign_body(seed, data)


def public_key_base64(seed: bytes) -> str:
    key = _private_key(seed)
    pub_bytes = key.public_key().public_bytes_raw()
    return base64.b64encode(pub_bytes).decode()


def public_key_hex(seed: bytes) -> str:
    key = _private_key(seed)
    pub_bytes = key.public_key().public_bytes_raw()
    return pub_bytes.hex()
