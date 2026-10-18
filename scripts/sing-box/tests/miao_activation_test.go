package main

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"os"
	"os/exec"
	"path/filepath"
	"syscall"
	"testing"
	"time"

	"github.com/sagernet/sing-box/include"
)

func miaoWriteActivation(t *testing.T, directory, id, config string) {
	t.Helper()
	digest := sha256.Sum256([]byte(config))
	request := miaoActivationRequest{1, id, hex.EncodeToString(digest[:])}
	bytes, err := json.Marshal(request)
	if err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(filepath.Join(directory, "config.json"), []byte(config), 0600); err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(filepath.Join(directory, "activation-request.json"), bytes, 0600); err != nil {
		t.Fatal(err)
	}
}

func TestMiaoActivationRejectsMismatchedConfigAndKeepsCapturedRequest(t *testing.T) {
	previousCtx, previousPaths, previousDirs := globalCtx, configPaths, configDirectories
	t.Cleanup(func() { globalCtx, configPaths, configDirectories = previousCtx, previousPaths, previousDirs })
	globalCtx = include.Context(context.Background())
	directory := t.TempDir()
	t.Setenv("MIAO_ACTIVATION_DIR", directory)
	configPaths, configDirectories = []string{filepath.Join(directory, "config.json")}, nil
	config := `{"log":{"disabled":true},"outbounds":[{"type":"direct","tag":"direct"}],"route":{"final":"direct"}}`
	miaoWriteActivation(t, directory, "first", config)
	_, first, err := readMiaoConfig()
	if err != nil {
		t.Fatal(err)
	}
	miaoWriteActivation(t, directory, "second", config)
	if err = first.publish(); err != nil {
		t.Fatal(err)
	}
	bytes, err := os.ReadFile(filepath.Join(directory, "activation-ack.json"))
	if err != nil {
		t.Fatal(err)
	}
	var ack struct {
		miaoActivationRequest
		PID int `json:"pid"`
	}
	if err = json.Unmarshal(bytes, &ack); err != nil {
		t.Fatal(err)
	}
	if ack.RequestID != "first" || ack.PID != os.Getpid() {
		t.Fatalf("old create acknowledged a newer request: %+v", ack)
	}
	if err = os.WriteFile(configPaths[0], []byte(config+"\n"), 0600); err != nil {
		t.Fatal(err)
	}
	if _, _, err = readMiaoConfig(); err == nil {
		t.Fatal("mismatched config hash accepted")
	}
}

// A subprocess exercises the actual run/SIGHUP hooks, without TUN, listeners,
// or external network. Signals must never be sent to the parent Go test process.
func TestMiaoActivationRunHelper(t *testing.T) {
	if os.Getenv("MIAO_ACTIVATION_TEST_CHILD") != "1" {
		return
	}
	globalCtx = include.Context(context.Background())
	configPaths, configDirectories = []string{filepath.Join(os.Getenv("MIAO_ACTIVATION_DIR"), "config.json")}, nil
	if err := run(); err != nil {
		t.Fatal(err)
	}
}

func TestMiaoActivationRunAcknowledgesOnlySuccessfulReloads(t *testing.T) {
	directory := t.TempDir()
	config := `{"log":{"disabled":true},"outbounds":[{"type":"direct","tag":"direct"}],"route":{"final":"direct"}}`
	miaoWriteActivation(t, directory, "start", config)
	command := exec.Command(os.Args[0], "-test.run=^TestMiaoActivationRunHelper$")
	command.Env = append(os.Environ(), "MIAO_ACTIVATION_TEST_CHILD=1", "MIAO_ACTIVATION_DIR="+directory)
	if err := command.Start(); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = command.Process.Kill(); _ = command.Wait() })
	readAck := func() string {
		bytes, err := os.ReadFile(filepath.Join(directory, "activation-ack.json"))
		if err != nil {
			return ""
		}
		var ack struct {
			miaoActivationRequest
			PID int `json:"pid"`
		}
		if json.Unmarshal(bytes, &ack) != nil || ack.PID != command.Process.Pid {
			return ""
		}
		return ack.RequestID
	}
	waitAck := func(expected string) {
		t.Helper()
		deadline := time.Now().Add(5 * time.Second)
		for time.Now().Before(deadline) {
			if readAck() == expected {
				return
			}
			time.Sleep(5 * time.Millisecond)
		}
		t.Fatalf("acknowledgement did not become %q, got %q", expected, readAck())
	}
	waitAck("start")
	miaoWriteActivation(t, directory, "reload", config+"\n")
	if err := command.Process.Signal(syscall.SIGHUP); err != nil {
		t.Fatal(err)
	}
	waitAck("reload")
	miaoWriteActivation(t, directory, "rejected", `{"outbounds":[{"type":"not-a-protocol"}]}`)
	if err := command.Process.Signal(syscall.SIGHUP); err != nil {
		t.Fatal(err)
	}
	time.Sleep(100 * time.Millisecond)
	if readAck() != "reload" {
		t.Fatal("failed reload published an acknowledgement")
	}
	if err := command.Process.Signal(syscall.Signal(0)); err != nil {
		t.Fatal("failed precheck did not keep the old instance", err)
	}
}
