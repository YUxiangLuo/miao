package clashapi

import (
	"bytes"
	"context"
	stdjson "encoding/json"
	"fmt"
	"net/http/httptest"
	"net/netip"
	"reflect"
	"sync/atomic"
	"testing"
	"time"

	"github.com/sagernet/sing-box/adapter"
	"github.com/sagernet/sing-box/common/trafficcontrol"
	C "github.com/sagernet/sing-box/constant"
	"github.com/sagernet/sing/common/json"
	M "github.com/sagernet/sing/common/metadata"
	"github.com/sagernet/ws"
	"github.com/sagernet/ws/wsutil"

	"github.com/gofrs/uuid/v5"
)

func miaoConnectionFixture() trafficcontrol.TrackerMetadata {
	upload, download := new(atomic.Int64), new(atomic.Int64)
	upload.Store(9007199254740993)
	download.Store(29)
	return trafficcontrol.TrackerMetadata{
		ID: uuid.FromStringOrNil("00112233-4455-4677-8899-aabbccddeeff"),
		Metadata: adapter.InboundContext{
			Network: "tcp", Inbound: "tun-in", InboundType: "tun",
			Source:      M.ParseSocksaddr("192.0.2.7:43210"),
			Destination: M.Socksaddr{Addr: netip.MustParseAddr("2001:db8::9"), Port: 443, Fqdn: "fallback.example"},
			Domain:      "例子.example<>&\"",
		},
		CreatedAt: time.Date(2026, 9, 23, 12, 34, 56, 123456789, time.FixedZone("test", 2*3600)),
		Upload:    upload, Download: download, Chain: []string{"proxy", "出口"},
	}
}

// UseNumber catches loss of int64 precision, while comparing JSON values permits
// object key ordering to change. Expected fields below are independent of the encoder.
func miaoDecodeJSON(t testing.TB, data []byte) map[string]any {
	t.Helper()
	var value map[string]any
	decoder := stdjson.NewDecoder(bytes.NewReader(data))
	decoder.UseNumber()
	if err := decoder.Decode(&value); err != nil {
		t.Fatal(err)
	}
	return value
}

func miaoEncodeConnection(c *trafficcontrol.TrackerMetadata) ([]byte, error) {
	return json.Marshal(newConnectionObject(c))
}

func TestMiaoConnectionJSON(t *testing.T) {
	c := miaoConnectionFixture()
	data, err := miaoEncodeConnection(&c)
	if err != nil {
		t.Fatal(err)
	}
	want := miaoDecodeJSON(t, []byte(`{"id":"00112233-4455-4677-8899-aabbccddeeff","metadata":{"network":"tcp","type":"tun/tun-in","sourceIP":"192.0.2.7","destinationIP":"2001:db8::9","sourcePort":"43210","destinationPort":"443","host":"例子.example<>&\"","dnsMode":"normal","processPath":""},"upload":9007199254740993,"download":29,"start":"2026-09-23T12:34:56.123456789+02:00","chains":["proxy","出口"],"rule":"final","rulePayload":""}`))
	if got := miaoDecodeJSON(t, data); !reflect.DeepEqual(got, want) {
		t.Fatalf("connection JSON mismatch: got %s, want %#v", data, want)
	}
	for _, tc := range []struct {
		name  string
		owner *adapter.ConnectionOwner
		want  string
	}{
		{"absent", nil, ""},
		{"unknown", &adapter.ConnectionOwner{UserId: -1}, ""},
		{"uid-zero", &adapter.ConnectionOwner{UserId: 0}, "0"},
		{"uid", &adapter.ConnectionOwner{UserId: 1234, UserName: "ignored"}, "1234"},
		{"path", &adapter.ConnectionOwner{UserId: -1, ProcessPaths: []string{"/app/browser", "/ignored"}}, "/app/browser"},
		{"user-name", &adapter.ConnectionOwner{UserId: 1234, UserName: "用户", ProcessPaths: []string{"/app/browser"}, PackageNames: []string{"ignored"}}, "/app/browser (用户)"},
		{"package", &adapter.ConnectionOwner{UserId: 1234, PackageNames: []string{"com.example.app", "ignored"}}, "com.example.app (1234)"},
		{"empty-first-path", &adapter.ConnectionOwner{UserId: 42, ProcessPaths: []string{""}, PackageNames: []string{"ignored"}}, "42"},
	} {
		t.Run(tc.name, func(t *testing.T) {
			c.Metadata.ProcessInfo = tc.owner
			c.Metadata.Domain = ""
			c.Metadata.Inbound = ""
			c.Metadata.Source = M.Socksaddr{}
			c.Metadata.Destination.Addr = netip.Addr{}
			c.Chain = nil
			c.Rule = miaoConnectionRule{}
			data, err := miaoEncodeConnection(&c)
			if err != nil {
				t.Fatal(err)
			}
			want["chains"] = nil
			want["rule"] = "domain=example.org => route(proxy)"
			metadata := want["metadata"].(map[string]any)
			metadata["type"], metadata["host"], metadata["processPath"] = "tun", "fallback.example", tc.want
			metadata["sourceIP"], metadata["destinationIP"], metadata["sourcePort"] = "", "", "0"
			if got := miaoDecodeJSON(t, data); !reflect.DeepEqual(got, want) {
				t.Fatalf("got %s, want %#v", data, want)
			}
		})
	}
	c.Chain = []string{}
	data, err = miaoEncodeConnection(&c)
	if err != nil || !reflect.DeepEqual(miaoDecodeJSON(t, data)["chains"], []any{}) {
		t.Fatalf("empty chains must remain [], got %s, %v", data, err)
	}
}

type miaoConnectionRule struct{ adapter.Rule }

func (miaoConnectionRule) String() string             { return "domain=example.org" }
func (miaoConnectionRule) Action() adapter.RuleAction { return miaoConnectionAction{} }

type miaoConnectionAction struct{ adapter.RuleAction }

func (miaoConnectionAction) String() string { return "route(proxy)" }

type miaoConnectionOutbound struct {
	adapter.Outbound
	kind string
}

func (o miaoConnectionOutbound) Tag() string  { return o.kind }
func (o miaoConnectionOutbound) Type() string { return o.kind }

// Both HTTP and WebSocket use the real snapshot path. DNS is hidden from the
// connection list but still included in totals, including after it closes.
func TestMiaoConnectionsTransports(t *testing.T) {
	manager := trafficcontrol.NewManager()
	if err := manager.Start(adapter.StartStateInitialize); err != nil {
		t.Fatal(err)
	}
	defer manager.Close()
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	server := httptest.NewServer(connectionRouter(ctx, nil, manager))
	defer server.Close()
	readHTTP := func() map[string]any {
		response, err := server.Client().Get(server.URL + "/")
		if err != nil {
			t.Fatal(err)
		}
		defer response.Body.Close()
		if response.StatusCode != 200 {
			t.Fatal(response.Status)
		}
		var buf bytes.Buffer
		if _, err = buf.ReadFrom(response.Body); err != nil {
			t.Fatal(err)
		}
		return miaoDecodeJSON(t, buf.Bytes())
	}
	if got := readHTTP()["connections"]; !reflect.DeepEqual(got, []any{}) {
		t.Fatalf("empty snapshot changed: %#v", got)
	}
	for _, tc := range []struct {
		kind     string
		up, down int
	}{{C.TypeDirect, 37, 91}, {C.TypeDNS, 5, 11}} {
		metadata := miaoConnectionFixture().Metadata
		outbound := miaoConnectionOutbound{kind: tc.kind}
		metadata.OutboundChain = []adapter.Outbound{outbound}
		flow := manager.RoutedFlow(ctx, metadata, nil, outbound)
		flow.AttachFlow(nil)
		flow.CountForward(tc.up)
		flow.CountReverse(tc.down)
	}
	httpSnapshot := readHTTP()
	if httpSnapshot["uploadTotal"] != stdjson.Number("42") || httpSnapshot["downloadTotal"] != stdjson.Number("102") {
		t.Fatalf("wrong totals: %#v", httpSnapshot)
	}
	if list := httpSnapshot["connections"].([]any); len(list) != 1 || list[0].(map[string]any)["upload"] != stdjson.Number("37") {
		t.Fatalf("wrong filtered connections: %#v", list)
	}
	conn, _, _, err := ws.Dial(ctx, "ws"+server.URL[4:]+"/?interval=10")
	if err != nil {
		t.Fatal(err)
	}
	defer conn.Close()
	if err := conn.SetReadDeadline(time.Now().Add(5 * time.Second)); err != nil {
		t.Fatal(err)
	}
	for range 2 {
		data, _, err := wsutil.ReadServerData(conn)
		if err != nil {
			t.Fatal(err)
		}
		got := miaoDecodeJSON(t, data)
		delete(got, "memory")
		want := readHTTP()
		delete(want, "memory")
		if !reflect.DeepEqual(got, want) {
			t.Fatalf("HTTP/WS differ: %s vs %#v", data, want)
		}
	}
	manager.CloseAllConnections()
	closed := readHTTP()
	if !reflect.DeepEqual(closed["connections"], []any{}) || closed["uploadTotal"] != stdjson.Number("42") || closed["downloadTotal"] != stdjson.Number("102") {
		t.Fatalf("closed accounting changed: %#v", closed)
	}
}

// Measures snapshot conversion + JSON encoding, not network throughput or /proc.
// Run unchanged fixtures before/after the patch with the same pinned toolchain.
func BenchmarkMiaoConnections(b *testing.B) {
	for _, count := range []int{1, 100, 1000, 10000} {
		b.Run(fmt.Sprint(count), func(b *testing.B) {
			metadata := make([]trafficcontrol.TrackerMetadata, count)
			for i := range metadata {
				metadata[i] = miaoConnectionFixture()
				metadata[i].Metadata.Domain = fmt.Sprintf("host-%d.example.org", i)
				metadata[i].Metadata.ProcessInfo = &adapter.ConnectionOwner{UserId: 1000, ProcessPaths: []string{"/usr/bin/browser"}}
			}
			b.ReportAllocs()
			b.ResetTimer()
			for range b.N {
				connections := make([]connectionObject, count)
				for i := range metadata {
					connections[i] = newConnectionObject(&metadata[i])
				}
				_, err := json.Marshal(map[string]any{"uploadTotal": int64(37), "downloadTotal": int64(91), "connections": connections, "memory": int64(123456)})
				if err != nil {
					b.Fatal(err)
				}
			}
		})
	}
}
