package main

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"io"
	"net"
	"testing"
	"time"

	server "github.com/sagernet/sing-shadowsocks"
	"github.com/sagernet/sing-shadowsocks/shadowaead"
	aead2022 "github.com/sagernet/sing-shadowsocks/shadowaead_2022"
	client "github.com/sagernet/sing-shadowsocks2"
	M "github.com/sagernet/sing/common/metadata"
	N "github.com/sagernet/sing/common/network"
)

var harborAEADMethods = []string{"aes-128-gcm", "2022-blake3-aes-128-gcm"}

type harborEOFHandler struct {
	server.Handler
	handle func(net.Conn, M.Metadata) error
}

func (h harborEOFHandler) NewConnection(_ context.Context, conn net.Conn, metadata M.Metadata) error {
	return h.handle(conn, metadata)
}

func harborAEADClient(t *testing.T, name string) client.Method {
	t.Helper()
	key := []byte("0123456789abcdef")
	method, err := client.CreateMethod(context.Background(), name, client.MethodOptions{Key: key, KeyList: [][]byte{key}})
	if err != nil {
		t.Fatal(err)
	}
	return method
}

func harborCloseWrite(t *testing.T, conn net.Conn) error {
	t.Helper()
	closer, ok := conn.(N.WriteCloser)
	if !ok {
		t.Fatal("AEAD wrapper hides transport CloseWrite")
	}
	return closer.CloseWrite()
}

// Real TCP, no TUN or external network. The independent server implementation
// replies only AFTER authenticated request EOF; closing both halves loses it.
func TestHarborShadowsocksHalfClose(t *testing.T) {
	for _, name := range harborAEADMethods {
		for _, size := range []int{0, 131071} {
			t.Run(fmt.Sprintf("%s/%d", name, size), func(t *testing.T) {
				request := make([]byte, size)
				for i := range request {
					request[i] = byte(i % 251)
				}
				response := bytes.Repeat([]byte("response-after-EOF!"), 4097)
				destination := M.ParseSocksaddr("example.test:4321")
				handler := harborEOFHandler{handle: func(conn net.Conn, metadata M.Metadata) error {
					if metadata.Destination != destination {
						return fmt.Errorf("wrong destination: %v", metadata.Destination)
					}
					got, err := io.ReadAll(conn)
					if err != nil {
						return err
					}
					if !bytes.Equal(got, request) {
						return fmt.Errorf("request mismatch: got %d bytes, want %d", len(got), size)
					}
					// Match a relay's bounded copy buffer, including its first write.
					_, err = io.Copy(conn, io.LimitReader(bytes.NewReader(response), int64(len(response))))
					return err
				}}
				var service server.Service
				var err error
				if name == "aes-128-gcm" {
					service, err = shadowaead.NewService(name, []byte("0123456789abcdef"), "", 60, handler)
				} else {
					service, err = aead2022.NewService(name, []byte("0123456789abcdef"), 60, handler, nil)
				}
				if err != nil {
					t.Fatal(err)
				}
				listener, err := net.ListenTCP("tcp4", &net.TCPAddr{IP: net.IPv4(127, 0, 0, 1)})
				if err != nil {
					t.Fatal(err)
				}
				t.Cleanup(func() { listener.Close() })
				raw, err := net.DialTimeout("tcp4", listener.Addr().String(), 3*time.Second)
				if err != nil {
					t.Fatal(err)
				}
				t.Cleanup(func() { raw.Close() })
				peer, err := listener.AcceptTCP()
				if err != nil {
					t.Fatal(err)
				}
				raw.SetDeadline(time.Now().Add(3 * time.Second))
				peer.SetDeadline(time.Now().Add(3 * time.Second))
				done := make(chan error, 1)
				go func() {
					err := service.NewConnection(context.Background(), peer, M.Metadata{})
					peer.Close()
					done <- err
				}()
				t.Cleanup(func() {
					peer.Close()
					if err := <-done; err != nil {
						t.Errorf("server: %v", err)
					}
				})
				conn := harborAEADClient(t, name).DialEarlyConn(raw, destination)
				t.Cleanup(func() { conn.Close() })
				if size > 0 {
					if _, err := conn.Write(request); err != nil {
						t.Fatal(err)
					}
				}
				// For size=0, CloseWrite itself must send the destination request.
				if err := harborCloseWrite(t, conn); err != nil {
					t.Fatal(err)
				}
				got, err := io.ReadAll(conn)
				if err != nil || !bytes.Equal(got, response) {
					t.Fatalf("response: got %d bytes, want %d, error %v", len(got), len(response), err)
				}
				if _, err := conn.Write([]byte("late write")); err == nil {
					t.Fatal("write after FIN succeeded")
				}
			})
		}
	}
}

// Opaque transports intentionally expose Upstream without WriterReplaceable.
// Reaching past one would bypass a plugin's transport shutdown contract.
type harborOpaqueTransport struct {
	net.Conn
	writes, closes int
	writeErr       error
}

func (c *harborOpaqueTransport) Write(p []byte) (int, error) {
	c.writes++
	if c.writeErr != nil {
		return 0, c.writeErr
	}
	return len(p), nil
}

func (c *harborOpaqueTransport) Close() error  { c.closes++; return nil }
func (c *harborOpaqueTransport) Upstream() any { return c.Conn }

type harborDuplexTransport struct {
	harborOpaqueTransport
	fins   int
	finErr error
}

func (c *harborDuplexTransport) CloseWrite() error { c.fins++; return c.finErr }

type harborTransparentTransport struct{ net.Conn }

func (c harborTransparentTransport) WriterReplaceable() bool { return true }
func (c harborTransparentTransport) Upstream() any           { return c.Conn }

func TestHarborShadowsocksHalfCloseTransportContracts(t *testing.T) {
	failure := errors.New("injected transport failure")
	for _, name := range harborAEADMethods {
		t.Run(name, func(t *testing.T) {
			method := harborAEADClient(t, name)
			destination := M.ParseSocksaddr("example.test:4321")
			t.Run("opaque", func(t *testing.T) {
				underlying := &harborDuplexTransport{}
				transport := &harborOpaqueTransport{Conn: underlying}
				conn := method.DialEarlyConn(transport, destination)
				if err := harborCloseWrite(t, conn); err != nil {
					t.Fatal(err)
				}
				if transport.closes != 1 || transport.writes != 0 || underlying.fins != 0 {
					t.Fatal("unsupported transport must fully close without emitting a request or bypassing the wrapper")
				}
			})
			t.Run("request-error", func(t *testing.T) {
				transport := &harborDuplexTransport{harborOpaqueTransport: harborOpaqueTransport{writeErr: failure}}
				conn := method.DialEarlyConn(transport, destination)
				if err := harborCloseWrite(t, conn); !errors.Is(err, failure) || transport.fins != 0 {
					t.Fatalf("request error must propagate before FIN: %v", err)
				}
			})
			t.Run("transparent-fin-error", func(t *testing.T) {
				transport := &harborDuplexTransport{finErr: failure}
				conn := method.DialEarlyConn(harborTransparentTransport{transport}, destination)
				if _, err := conn.Write([]byte("initialized")); err != nil {
					t.Fatal(err)
				}
				writes := transport.writes
				if err := harborCloseWrite(t, conn); !errors.Is(err, failure) || transport.fins != 1 || transport.writes != writes || transport.closes != 0 {
					t.Fatalf("initialized wrapper must delegate FIN without re-handshake or full close: %v", err)
				}
			})
		})
	}
}
