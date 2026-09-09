package main

import (
	"encoding/json"
	"os"
	"slices"
	"testing"

	"github.com/sagernet/sing-box/include"
)

type miaoRegistrySnapshot struct {
	Outbounds []string
	Endpoints []string
	DNS       []string
}

// The exact supported set comes from the shared profile. Also verify that
// every retained type exists in the unmodified pinned upstream build.
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
	profileData, err := os.ReadFile(os.Getenv("MIAO_PROFILE_MANIFEST"))
	if err != nil {
		t.Fatal(err)
	}
	var profile struct {
		NodeProtocols []string `json:"node_protocols"`
		DNSTransports []string `json:"dns_transports"`
	}
	if err := json.Unmarshal(profileData, &profile); err != nil {
		t.Fatal(err)
	}
	expectedOutbounds := append(slices.Clone(profile.NodeProtocols), "direct", "selector", "urltest")
	slices.Sort(expectedOutbounds)
	slices.Sort(profile.DNSTransports)
	if !slices.Equal(got.Outbounds, expectedOutbounds) || !slices.Equal(got.DNS, profile.DNSTransports) || len(got.Endpoints) != 0 {
		t.Fatalf("client registries differ from the profile: got %+v, expected outbounds=%v DNS=%v and no endpoints", got, expectedOutbounds, profile.DNSTransports)
	}
	for _, retained := range got.Outbounds {
		if !slices.Contains(want.Outbounds, retained) {
			t.Fatalf("retained outbound %s is absent upstream", retained)
		}
	}
	for _, retained := range got.DNS {
		if !slices.Contains(want.DNS, retained) {
			t.Fatalf("retained DNS transport %s is absent upstream", retained)
		}
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
