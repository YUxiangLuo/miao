//go:build !windows

package main

import (
	"syscall"
	"testing"
	"time"
)

func TestHarborActivationRunAcknowledgesOnlySuccessfulReloads(t *testing.T) {
	directory := t.TempDir()
	config := `{"log":{"disabled":true},"outbounds":[{"type":"direct","tag":"direct"}],"route":{"final":"direct"}}`
	harborWriteActivation(t, directory, "start", config)
	command, readAck, waitAck := harborStartActivationChild(t, directory)
	waitAck("start")
	harborWriteActivation(t, directory, "reload", config+"\n")
	if err := command.Process.Signal(syscall.SIGHUP); err != nil {
		t.Fatal(err)
	}
	waitAck("reload")
	harborWriteActivation(t, directory, "rejected", `{"outbounds":[{"type":"not-a-protocol"}]}`)
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
