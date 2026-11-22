package main

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"os"
	"os/exec"
	"path/filepath"
	"testing"
	"time"

	"github.com/sagernet/sing-box/include"
)

func harborWriteActivation(t *testing.T, directory, id, config string) {
	t.Helper()
	digest := sha256.Sum256([]byte(config))
	request := harborActivationRequest{1, id, hex.EncodeToString(digest[:])}
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

func TestHarborActivationRejectsMismatchedConfigAndKeepsCapturedRequest(t *testing.T) {
	previousCtx, previousPaths, previousDirs := globalCtx, configPaths, configDirectories
	t.Cleanup(func() { globalCtx, configPaths, configDirectories = previousCtx, previousPaths, previousDirs })
	globalCtx = include.Context(context.Background())
	directory := t.TempDir()
	t.Setenv("HARBOR_ACTIVATION_DIR", directory)
	configPaths, configDirectories = []string{filepath.Join(directory, "config.json")}, nil
	config := `{"log":{"disabled":true},"outbounds":[{"type":"direct","tag":"direct"}],"route":{"final":"direct"}}`
	harborWriteActivation(t, directory, "first", config)
	_, first, err := readHarborConfig()
	if err != nil {
		t.Fatal(err)
	}
	harborWriteActivation(t, directory, "second", config)
	if err = first.publish(); err != nil {
		t.Fatal(err)
	}
	bytes, err := os.ReadFile(filepath.Join(directory, "activation-ack.json"))
	if err != nil {
		t.Fatal(err)
	}
	var ack struct {
		harborActivationRequest
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
	if _, _, err = readHarborConfig(); err == nil {
		t.Fatal("mismatched config hash accepted")
	}
}

// A subprocess exercises the actual run/SIGHUP hooks, without TUN, listeners,
// or external network. Signals must never be sent to the parent Go test process.
func TestHarborActivationRunHelper(t *testing.T) {
	if os.Getenv("HARBOR_ACTIVATION_TEST_CHILD") != "1" {
		return
	}
	globalCtx = include.Context(context.Background())
	configPaths, configDirectories = []string{filepath.Join(os.Getenv("HARBOR_ACTIVATION_DIR"), "config.json")}, nil
	if err := run(); err != nil {
		t.Fatal(err)
	}
}

func harborStartActivationChild(t *testing.T, directory string) (*exec.Cmd, func() string, func(string)) {
	t.Helper()
	command := exec.Command(os.Args[0], "-test.run=^TestHarborActivationRunHelper$")
	command.Env = append(os.Environ(), "HARBOR_ACTIVATION_TEST_CHILD=1", "HARBOR_ACTIVATION_DIR="+directory)
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
			harborActivationRequest
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
	return command, readAck, waitAck
}

func TestHarborActivationRunPublishesInitialAcknowledgement(t *testing.T) {
	directory := t.TempDir()
	config := `{"log":{"disabled":true},"outbounds":[{"type":"direct","tag":"direct"}],"route":{"final":"direct"}}`
	harborWriteActivation(t, directory, "start", config)
	_, _, waitAck := harborStartActivationChild(t, directory)
	waitAck("start")
}
