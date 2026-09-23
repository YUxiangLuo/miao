package log

import (
	"bytes"
	"context"
	"encoding/json"
	"io"
	"strings"
	"testing"
	"time"

	F "github.com/sagernet/sing/common/format"
)

// Unlike io.Discard itself, this exercises the real console formatter.
type miaoDiscardWriter struct{}

func (miaoDiscardWriter) Write(p []byte) (int, error)       { return len(p), nil }
func (miaoDiscardWriter) WriteString(s string) (int, error) { return len(s), nil }

type miaoByteWriter struct{ io.Writer }

type miaoBlockingWriter struct{ entered, release chan struct{} }

func (w miaoBlockingWriter) Write(p []byte) (int, error) { return w.WriteString(string(p)) }
func (w miaoBlockingWriter) WriteString(s string) (int, error) {
	close(w.entered)
	<-w.release
	return len(s), nil
}

func TestMiaoLogPreservesBackpressure(t *testing.T) {
	w := miaoBlockingWriter{make(chan struct{}), make(chan struct{})}
	factory := NewDefaultFactory(context.Background(), Formatter{DisableColors: true}, w, "", nil, false)
	if err := factory.Start(); err != nil {
		t.Fatal(err)
	}
	defer factory.Close()
	done := make(chan struct{})
	go func() { factory.Logger().ErrorContext(context.Background(), "blocked error"); close(done) }()
	select {
	case <-w.entered:
	case <-time.After(time.Second):
		t.Fatal("writer was not called")
	}
	select {
	case <-done:
		t.Error("logging returned before synchronous write completed")
	case <-time.After(20 * time.Millisecond):
	}
	close(w.release)
	select {
	case <-done:
	case <-time.After(time.Second):
		t.Fatal("logging did not return after write")
	}
}

func TestMiaoLogTextAndWriterCompatibility(t *testing.T) {
	for _, stringWriter := range []bool{false, true} {
		var output bytes.Buffer
		var writer io.Writer = &output
		if !stringWriter {
			writer = miaoByteWriter{writer}
		}
		factory := NewDefaultFactory(context.Background(), Formatter{DisableColors: true, DisableTimestamp: true}, writer, "", nil, false)
		logger := factory.NewLogger("dns")
		// Include queued-before-Start messages, empty text and non-string args.
		logger.InfoContext(context.Background(), "before start")
		if err := factory.Start(); err != nil {
			t.Fatal(err)
		}
		t.Cleanup(func() { _ = factory.Close() })
		args := [][]any{{""}, {"single\nline"}, {123}, {"count=", 42, "/", true}, {"already terminated\n"}}
		want := "INFO dns: before start\n"
		for _, values := range args {
			logger.InfoContext(context.Background(), values...)
			message := F.ToString(values...)
			want += "INFO dns: " + message
			if !strings.HasSuffix(message, "\n") {
				want += "\n"
			}
		}
		if got := output.String(); got != want {
			t.Fatalf("StringWriter=%v\ngot %q\nwant %q", stringWriter, got, want)
		}
	}
}

func BenchmarkMiaoLog(b *testing.B) {
	for _, sink := range []string{"string-writer", "byte-writer"} {
		b.Run(sink, func(b *testing.B) {
			var writer io.Writer = miaoDiscardWriter{}
			if sink == "byte-writer" {
				writer = miaoByteWriter{writer}
			}
			factory := NewDefaultFactory(context.Background(), Formatter{DisableColors: true, DisableTimestamp: true}, writer, "", nil, false)
			if err := factory.Start(); err != nil {
				b.Fatal(err)
			}
			defer factory.Close()
			logger := factory.NewLogger("dns")
			ctx := context.Background()
			b.Run("plain", func(b *testing.B) {
				b.ReportAllocs()
				for b.Loop() {
					logger.TraceContext(ctx, "exchange example.com IN A")
				}
			})
			b.Run("mixed", func(b *testing.B) {
				b.ReportAllocs()
				for b.Loop() {
					logger.TraceContext(ctx, "exchange ", "example.com", " type=", 28)
				}
			})
			// Typed JSON, including escaping, like Miao's DNS completion event.
			event := struct {
				Version   int     `json:"v"`
				Domain    string  `json:"domain"`
				QueryType string  `json:"query_type"`
				Duration  float64 `json:"duration_ms"`
				Outcome   string  `json:"outcome"`
				Rcode     *int    `json:"rcode"`
				Error     *string `json:"error"`
				Transport *string `json:"transport"`
			}{Version: 1, Domain: "quote\".example", QueryType: "A", Duration: 12.345, Outcome: "success", Rcode: new(int), Transport: new(string)}
			for _, joined := range []bool{false, true} {
				name := "dns-separate"
				if joined {
					name = "dns-joined"
				}
				b.Run(name, func(b *testing.B) {
					b.ReportAllocs()
					for b.Loop() {
						encoded, err := json.Marshal(event)
						if err != nil {
							b.Fatal(err)
						}
						if joined {
							logger.InfoContext(ctx, "MIAO_DNS "+string(encoded))
						} else {
							logger.InfoContext(ctx, "MIAO_DNS ", string(encoded))
						}
					}
				})
			}
		})
	}
}
