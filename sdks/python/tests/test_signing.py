import base64

from oris_sdk.signing import public_key_base64, public_key_hex, sign_body, sign_payload


SEED = b"test-seed-32-bytes-for-testing!!"


def test_sign_body_returns_base64():
    sig = sign_body(SEED, b'{"hello":"world"}')
    raw = base64.b64decode(sig)
    assert len(raw) == 64


def test_sign_payload_deterministic():
    payload = {"gene_id": "g1", "confidence": 0.9}
    sig1 = sign_payload(SEED, payload)
    sig2 = sign_payload(SEED, payload)
    assert sig1 == sig2
    assert len(base64.b64decode(sig1)) == 64


def test_public_key_base64_length():
    pk = public_key_base64(SEED)
    raw = base64.b64decode(pk)
    assert len(raw) == 32


def test_public_key_hex_length():
    pk = public_key_hex(SEED)
    assert len(pk) == 64
    int(pk, 16)  # validates hex
