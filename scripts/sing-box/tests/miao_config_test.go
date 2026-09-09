package main

import (
	"bytes"
	"encoding/json"
	"os"
	"path/filepath"
	"testing"

	"github.com/sagernet/sing-box/common/srs"
	"github.com/sagernet/sing-box/option"
)

func TestMiaoClientConfigurations(t *testing.T) {
	// Synthetic credentials and loopback destinations only. check() constructs
	// the configuration without starting listeners, TUN or proxy connections.
	cases := map[string]string{
		"shadowsocks":   `{"type":"shadowsocks","method":"aes-128-gcm","password":"test"}`,
		"vmess":         `{"type":"vmess","uuid":"00000000-0000-4000-8000-000000000001","security":"auto"}`,
		"vless":         `{"type":"vless","uuid":"00000000-0000-4000-8000-000000000001"}`,
		"trojan":        `{"type":"trojan","password":"test","tls":{"enabled":true}}`,
		"anytls":        `{"type":"anytls","password":"test","tls":{"enabled":true}}`,
		"hysteria2":     `{"type":"hysteria2","password":"test","tls":{"enabled":true},"obfs":{"type":"gecko","password":"test"}}`,
		"tuic":          `{"type":"tuic","uuid":"00000000-0000-4000-8000-000000000001","password":"test","tls":{"enabled":true}}`,
		"manual-socks":  `{"type":"socks"}`,
		"manual-http":   `{"type":"http"}`,
		"vless-ws":      `{"type":"vless","uuid":"00000000-0000-4000-8000-000000000001","tls":{"enabled":true},"transport":{"type":"ws"}}`,
		"vless-grpc":    `{"type":"vless","uuid":"00000000-0000-4000-8000-000000000001","tls":{"enabled":true},"transport":{"type":"grpc"}}`,
		"vless-http":    `{"type":"vless","uuid":"00000000-0000-4000-8000-000000000001","tls":{"enabled":true},"transport":{"type":"http"}}`,
		"vless-reality": `{"type":"vless","uuid":"00000000-0000-4000-8000-000000000001","flow":"xtls-rprx-vision","tls":{"enabled":true,"server_name":"example.com","utls":{"enabled":true,"fingerprint":"chrome"},"reality":{"enabled":true,"public_key":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA","short_id":"abcd"}}}`,
	}
	for name, nodeJSON := range cases {
		t.Run(name, func(t *testing.T) {
			miaoContextFixture(t)
			var node map[string]any
			if err := json.Unmarshal([]byte(nodeJSON), &node); err != nil {
				t.Fatal(err)
			}
			node["tag"], node["server"], node["server_port"] = "node", "127.0.0.1", 1
			var config map[string]any
			if err := json.Unmarshal([]byte(`{
				"log":{"disabled":true},
				"experimental":{"clash_api":{"external_controller":"127.0.0.1:0"}},
				"dns":{"final":"remote","strategy":"ipv4_only","reverse_mapping":true,"cache_capacity":4096,
					"optimistic":{"enabled":true,"timeout":"8h"},
					"servers":[{"type":"https","tag":"remote","server":"1.1.1.1","detour":"proxy"},{"type":"udp","tag":"local","server":"223.5.5.5"}],
					"rules":[{"rule_set":["test-rules"],"action":"route","server":"local"}]},
				"route":{"final":"proxy","auto_detect_interface":true,"find_process":true,"default_domain_resolver":"local",
					"rules":[{"action":"sniff"},{"protocol":"dns","action":"hijack-dns"},{"rule_set":["test-rules"],"action":"route","outbound":"direct"}]}
			}`), &config); err != nil {
				t.Fatal(err)
			}
			config["outbounds"] = []any{
				map[string]any{"type": "direct", "tag": "direct"}, node,
				map[string]any{"type": "selector", "tag": "proxy", "outbounds": []string{"direct", "node"}},
				map[string]any{"type": "urltest", "tag": "auto", "outbounds": []string{"node"}, "url": "https://www.gstatic.com/generate_204"},
			}
			directory := filepath.Dir(configPaths[0])
			config["experimental"].(map[string]any)["cache_file"] = map[string]any{
				"enabled": true, "path": filepath.Join(directory, "cache.db"), "store_dns": true,
			}
			var rules option.PlainRuleSet
			if err := json.Unmarshal([]byte(`{"rules":[{"domain_suffix":["example.com"]}]}`), &rules); err != nil {
				t.Fatal(err)
			}
			var binaryRules bytes.Buffer
			if err := srs.Write(&binaryRules, rules, 4); err != nil {
				t.Fatal(err)
			}
			rulesPath := filepath.Join(directory, "test.srs")
			if err := os.WriteFile(rulesPath, binaryRules.Bytes(), 0600); err != nil {
				t.Fatal(err)
			}
			config["route"].(map[string]any)["rule_set"] = []any{
				map[string]any{"type": "local", "tag": "test-rules", "format": "binary", "path": rulesPath},
			}
			data, err := json.Marshal(config)
			if err != nil {
				t.Fatal(err)
			}
			if err := os.WriteFile(configPaths[0], data, 0600); err != nil {
				t.Fatal(err)
			}
			if err := check(); err != nil {
				t.Fatal(err)
			}
		})
	}
}

func TestMiaoClientCommands(t *testing.T) {
	if mainCommand.Name() != "miao-kernel" {
		t.Fatalf("unexpected command name: %s", mainCommand.Name())
	}
	for _, command := range mainCommand.Commands() {
		switch command.Name() {
		case "run", "check", "version", "netns-holder":
		default:
			t.Errorf("unexpected runtime command: %s", command.Name())
		}
	}
}
