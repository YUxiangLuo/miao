package sniff

import (
	"bytes"
	"context"
	"crypto/tls"
	"encoding/hex"
	"errors"
	"io"
	"net"
	"os"
	"testing"
	"time"

	"github.com/sagernet/sing-box/adapter"
	"github.com/sagernet/sing/common/buf"
)

type miaoSniffConn struct {
	net.Conn
	chunks          [][]byte
	output          bytes.Buffer
	readError       error
	deadlineError   error
	deadlines       []time.Time
	recordDeadlines bool
}

func (c *miaoSniffConn) Read(p []byte) (int, error) {
	if len(c.chunks) == 0 {
		if c.readError != nil {
			return 0, c.readError
		}
		return 0, io.EOF
	}
	n := copy(p, c.chunks[0])
	c.chunks[0] = c.chunks[0][n:]
	if len(c.chunks[0]) == 0 {
		c.chunks = c.chunks[1:]
	}
	return n, nil
}
func (c *miaoSniffConn) Write(p []byte) (int, error) { return c.output.Write(p) }
func (c *miaoSniffConn) SetReadDeadline(t time.Time) error {
	if c.recordDeadlines {
		c.deadlines = append(c.deadlines, t)
	}
	return c.deadlineError
}

var miaoStreamSniffers = []StreamSniffer{TLSClientHello, HTTPHost, StreamDomainNameQuery, BitTorrent, SSH, RDP}

func miaoClientHello(t testing.TB) []byte {
	t.Helper()
	c := new(miaoSniffConn)
	// No network or certificate verification occurs: capture ClientHello, then EOF.
	_ = tls.Client(c, &tls.Config{ServerName: "sni.example", MinVersion: tls.VersionTLS13}).Handshake()
	if c.output.Len() == 0 {
		t.Fatal("missing ClientHello")
	}
	return bytes.Clone(c.output.Bytes())
}

func TestMiaoSniffRealProtocolsAndCachedPrefixes(t *testing.T) {
	dns, err := hex.DecodeString("001e740701000001000000000000012a06676f6f676c6503636f6d0000010001")
	if err != nil {
		t.Fatal(err)
	}
	for _, tc := range []struct {
		name                     string
		payload                  []byte
		protocol, domain, client string
	}{
		{"http", []byte("GET /path HTTP/1.1\r\nHost: actual.example:8080\r\n\r\n"), "http", "actual.example", ""},
		{"tls", miaoClientHello(t), "tls", "sni.example", ""},
		{"dns", dns, "dns", "mapped.example", ""},
		{"ssh", []byte("SSH-2.0-MiaoTest\r\n"), "ssh", "mapped.example", "MiaoTest"},
	} {
		for _, mode := range []string{"uncached", "cached", "fragmented"} {
			t.Run(tc.name+"/"+mode, func(t *testing.T) {
				payload := bytes.Clone(tc.payload)
				var prefixes []*buf.Buffer
				if mode == "cached" {
					for _, part := range [][]byte{payload[:1], payload[1:3]} {
						b := buf.NewSize(len(part))
						_, _ = b.Write(part)
						defer b.Release()
						prefixes = append(prefixes, b)
					}
					payload = payload[3:]
				}
				b := buf.NewPacket()
				defer b.Release()
				chunks := [][]byte{payload}
				if mode == "fragmented" {
					chunks = [][]byte{payload[:1], payload[1:3], payload[3:]}
				}
				c := &miaoSniffConn{chunks: chunks, recordDeadlines: true}
				metadata := adapter.InboundContext{Domain: "mapped.example"}
				metadata.Destination.Port = 18443 // Do not narrow sniffing to standard ports.
				err := PeekStream(context.Background(), &metadata, c, prefixes, b, 0, miaoStreamSniffers...)
				if err != nil {
					t.Fatal(err)
				}
				if metadata.Protocol != tc.protocol || metadata.Domain != tc.domain || metadata.Client != tc.client {
					t.Fatalf("metadata: protocol=%q domain=%q client=%q", metadata.Protocol, metadata.Domain, metadata.Client)
				}
				if !bytes.Equal(b.Bytes(), payload) {
					t.Fatal("payload was consumed or modified")
				}
				if len(c.deadlines) != 2*len(chunks) || c.deadlines[0].IsZero() || !c.deadlines[len(c.deadlines)-1].IsZero() {
					t.Fatal("read deadline not cleared")
				}
			})
		}
	}
}

func TestMiaoSniffRewindsAndAccumulatesReads(t *testing.T) {
	b := buf.NewPacket()
	defer b.Release()
	c := &miaoSniffConn{chunks: [][]byte{[]byte("abc"), []byte("def")}, recordDeadlines: true}
	var seen []string
	reject := func(_ context.Context, _ *adapter.InboundContext, r io.Reader) error {
		p, _ := io.ReadAll(r)
		seen = append(seen, string(p))
		return os.ErrInvalid
	}
	accept := func(_ context.Context, _ *adapter.InboundContext, r io.Reader) error {
		p, _ := io.ReadAll(r)
		seen = append(seen, string(p))
		if string(p) == "abcdef" {
			return nil
		}
		return ErrNeedMoreData
	}
	if err := PeekStream(context.Background(), new(adapter.InboundContext), c, nil, b, time.Second, reject, accept); err != nil {
		t.Fatal(err)
	}
	if len(seen) != 4 || seen[0] != "abc" || seen[1] != "abc" || seen[2] != "abcdef" || seen[3] != "abcdef" {
		t.Fatalf("reader replay: %q", seen)
	}
	if string(b.Bytes()) != "abcdef" || len(c.deadlines) != 4 || !c.deadlines[0].Equal(c.deadlines[2]) || !c.deadlines[3].IsZero() {
		t.Fatal("buffer or absolute deadline changed")
	}
}

func TestMiaoSniffReadErrorsPreserved(t *testing.T) {
	for _, deadlineError := range []bool{false, true} {
		b := buf.NewPacket()
		c := &miaoSniffConn{readError: os.ErrDeadlineExceeded}
		if deadlineError {
			c.deadlineError = os.ErrPermission
		}
		err := PeekStream(context.Background(), new(adapter.InboundContext), c, nil, b, time.Second, miaoStreamSniffers...)
		b.Release()
		want := error(os.ErrDeadlineExceeded)
		if deadlineError {
			want = os.ErrPermission
		}
		if !errors.Is(err, want) {
			t.Fatalf("got %v, want %v", err, want)
		}
	}
}

func BenchmarkMiaoSniff(b *testing.B) {
	for _, tc := range []struct {
		name    string
		payload []byte
	}{
		{"http", []byte("GET /path HTTP/1.1\r\nHost: actual.example:8080\r\n\r\n")},
		{"tls", miaoClientHello(b)},
		{"ssh", []byte("SSH-2.0-MiaoTest\r\n")},
	} {
		for _, cached := range []bool{false, true} {
			name := tc.name + "/uncached"
			if cached {
				name = tc.name + "/cached"
			}
			b.Run(name, func(b *testing.B) {
				payload := tc.payload
				var prefixes []*buf.Buffer
				if cached {
					p := buf.NewSize(3)
					_, _ = p.Write(payload[:3])
					defer p.Release()
					prefixes = []*buf.Buffer{p}
					payload = payload[3:]
				}
				buffer := buf.NewPacket()
				defer buffer.Release()
				c := new(miaoSniffConn)
				chunks := make([][]byte, 1)
				b.ReportAllocs()
				for b.Loop() {
					buffer.Reset()
					chunks[0] = payload
					c.chunks = chunks
					var metadata adapter.InboundContext
					if err := PeekStream(context.Background(), &metadata, c, prefixes, buffer, 0, miaoStreamSniffers...); err != nil {
						b.Fatal(err)
					}
				}
			})
		}
	}
}
