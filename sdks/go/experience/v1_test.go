package experience

import (
	"encoding/json"
	"os"
	"path/filepath"
	"reflect"
	"testing"
)

func TestExperienceBundleV1GoldenRoundTrip(t *testing.T) {
	path := filepath.Join("..", "..", "..", "spec", "experience", "golden", "experience-bundle-v1.json")
	source, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	var bundle ExperienceBundleV1
	if err = json.Unmarshal(source, &bundle); err != nil {
		t.Fatal(err)
	}
	if err = bundle.Validate(); err != nil {
		t.Fatal(err)
	}
	encoded, err := json.Marshal(bundle)
	if err != nil {
		t.Fatal(err)
	}
	var want, got any
	_ = json.Unmarshal(source, &want)
	_ = json.Unmarshal(encoded, &got)
	if !reflect.DeepEqual(want, got) {
		t.Fatal("round trip lost fields")
	}
}
