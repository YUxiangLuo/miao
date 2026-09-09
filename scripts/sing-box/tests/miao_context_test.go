package main

import (
	"context"
	"os"
	"path/filepath"
	"testing"

	"github.com/sagernet/sing-box/adapter"
	"github.com/sagernet/sing-box/include"
	"github.com/sagernet/sing-box/option"
	"github.com/sagernet/sing/service"
)

// No listeners, TUN, subscription credentials, or remote connections. A check
// constructs an AnyTLS outbound but deliberately never initializes its client.
func miaoContextFixture(t *testing.T) option.Options {
	t.Helper()
	previousCtx, previousPaths, previousDirs := globalCtx, configPaths, configDirectories
	t.Cleanup(func() {
		globalCtx, configPaths, configDirectories = previousCtx, previousPaths, previousDirs
	})
	globalCtx = include.Context(context.Background())
	path := filepath.Join(t.TempDir(), "config.json")
	if err := os.WriteFile(path, []byte(`{
		"log":{"disabled":true},
		"outbounds":[
			{"type":"direct","tag":"direct"},
			{"type":"anytls","tag":"unused-anytls","server":"127.0.0.1","server_port":1,"password":"test","tls":{"enabled":true}}
		],
		"route":{"final":"direct"}
	}`), 0600); err != nil {
		t.Fatal(err)
	}
	configPaths, configDirectories = []string{path}, nil
	options, err := readConfigAndMerge()
	if err != nil {
		t.Fatal(err)
	}
	return options
}

func TestMiaoCheckIsolatesServiceRegistry(t *testing.T) {
	miaoContextFixture(t)
	if err := check(); err != nil {
		t.Fatal(err)
	}
	if service.FromContext[adapter.OutboundManager](globalCtx) != nil {
		t.Fatal("check published uninitialized outbounds into the shared CLI context")
	}
}

func TestMiaoCreateIsolatesServiceRegistry(t *testing.T) {
	options := miaoContextFixture(t)
	instance, cancel, err := create(options)
	if err != nil {
		t.Fatal(err)
	}
	defer instance.Close()
	defer cancel()
	if service.FromContext[adapter.OutboundManager](globalCtx) != nil {
		t.Fatal("running instance shares mutable managers with future checks/reloads")
	}
	// The in-process SIGHUP preflight must also leave the global context clean.
	if err := check(); err != nil {
		t.Fatal(err)
	}
	if service.FromContext[adapter.OutboundManager](globalCtx) != nil {
		t.Fatal("reload check leaked its temporary outbound manager")
	}
}
