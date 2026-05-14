package internal

import (
	"crypto/ed25519"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
)

func SignBody(seed [32]byte, body []byte) string {
	key := ed25519.NewKeyFromSeed(seed[:])
	sig := ed25519.Sign(key, body)
	return base64.StdEncoding.EncodeToString(sig)
}

func SignPayload(seed [32]byte, payload interface{}) (string, error) {
	data, err := json.Marshal(payload)
	if err != nil {
		return "", err
	}
	return SignBody(seed, data), nil
}

func PublicKeyBase64(seed [32]byte) string {
	key := ed25519.NewKeyFromSeed(seed[:])
	pub := key.Public().(ed25519.PublicKey)
	return base64.StdEncoding.EncodeToString(pub)
}

func PublicKeyHex(seed [32]byte) string {
	key := ed25519.NewKeyFromSeed(seed[:])
	pub := key.Public().(ed25519.PublicKey)
	return hex.EncodeToString(pub)
}
