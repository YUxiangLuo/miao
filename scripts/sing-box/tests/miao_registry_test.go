package main

import (
	"encoding/json"
	"os"
	"reflect"
	"slices"
	"testing"

	"github.com/sagernet/sing-box/include"
)

type miaoRegistrySnapshot struct {
	Outbounds []string
	Endpoints []string
	DNS       []string
}

// Compare the client profile against the unmodified pinned upstream build,
// including disabled-feature stubs and manual JSON node types. Removing an
// outbound by accident must not be masked by tests of just the YAML parser.
func TestMiaoRegistryCompatibility(t *testing.T) {
	got := miaoRegistrySnapshot{
		Outbounds: include.OutboundRegistry().OptionTypes(),
		Endpoints: include.EndpointRegistry().OptionTypes(),
		DNS:       include.DNSTransportRegistry().OptionTypes(),
	}
	path := os.Getenv("MIAO_REGISTRY_SNAPSHOT")
	if path == "" {
		t.Fatal("MIAO_REGISTRY_SNAPSHOT is required; run scripts/build-embedded.sh")
	}
	if os.Getenv("MIAO_CAPTURE_REGISTRIES") == "1" {
		data, err := json.Marshal(got)
		if err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(path, data, 0600); err != nil {
			t.Fatal(err)
		}
		return
	}
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	var want miaoRegistrySnapshot
	if err := json.Unmarshal(data, &want); err != nil {
		t.Fatal(err)
	}
	if !reflect.DeepEqual(got, want) {
		t.Fatalf("client changed upstream outbound/DNS/endpoint support:\nwant %+v\ngot  %+v", want, got)
	}
	if got := include.InboundRegistry().OptionTypes(); !slices.Equal(got, []string{"tun"}) {
		t.Fatalf("client must expose only TUN inbound, got %v", got)
	}
	if got := include.ServiceRegistry().OptionTypes(); len(got) != 0 {
		t.Fatalf("client unexpectedly includes services: %v", got)
	}
	if got := include.CertificateProviderRegistry().OptionTypes(); len(got) != 0 {
		t.Fatalf("client unexpectedly includes certificate issuers: %v", got)
	}
}
