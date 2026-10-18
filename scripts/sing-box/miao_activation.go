package main

// Miao's parent/child activation protocol is deliberately separate from Clash
// health: the previous instance can remain healthy during a slow reload check.
import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
)

type miaoActivationRequest struct {
	Version      int    `json:"version"`
	RequestID    string `json:"request_id"`
	ConfigSHA256 string `json:"config_sha256"`
}

type miaoActivation struct {
	miaoActivationRequest
	directory string
}

func readMiaoConfig() ([]*OptionsEntry, *miaoActivation, error) {
	// Capture ownership BEFORE reading configuration. A newer parent's request
	// cannot be acknowledged by an older create() still finishing its startup.
	var activation *miaoActivation
	if directory := os.Getenv("MIAO_ACTIVATION_DIR"); directory != "" {
		content, err := os.ReadFile(filepath.Join(directory, "activation-request.json"))
		if err != nil {
			return nil, nil, fmt.Errorf("read activation request: %w", err)
		}
		activation = &miaoActivation{directory: directory}
		if err = json.Unmarshal(content, &activation.miaoActivationRequest); err != nil {
			return nil, nil, fmt.Errorf("decode activation request: %w", err)
		}
		digest, err := hex.DecodeString(activation.ConfigSHA256)
		if activation.Version != 1 || activation.RequestID == "" || len(activation.RequestID) > 256 || err != nil || len(digest) != sha256.Size {
			return nil, nil, fmt.Errorf("invalid activation request")
		}
	}
	entries, err := readConfig()
	if err != nil {
		return nil, nil, err
	}
	if activation != nil {
		if len(entries) != 1 {
			return nil, nil, fmt.Errorf("managed activation requires one configuration file")
		}
		digest := sha256.Sum256(entries[0].content)
		if hex.EncodeToString(digest[:]) != activation.ConfigSHA256 {
			return nil, nil, fmt.Errorf("activation request does not match loaded configuration")
		}
	}
	return entries, activation, nil
}

// Called only after create() returns a successfully started instance. Failed
// checks/starts leave the previous acknowledgement, which has a different ID.
func (activation *miaoActivation) publish() error {
	if activation == nil {
		return nil // Standalone diagnostics and hermetic upstream-context tests.
	}
	ack := struct {
		miaoActivationRequest
		PID int `json:"pid"`
	}{activation.miaoActivationRequest, os.Getpid()}
	bytes, err := json.Marshal(ack)
	if err != nil {
		return err
	}
	file, err := os.CreateTemp(activation.directory, ".activation-ack-*")
	if err != nil {
		return err
	}
	defer os.Remove(file.Name())
	defer file.Close()
	if _, err = file.Write(bytes); err != nil {
		return err
	}
	if err = file.Sync(); err != nil {
		return err
	}
	if err = file.Close(); err != nil {
		return err
	}
	return os.Rename(file.Name(), filepath.Join(activation.directory, "activation-ack.json"))
}
