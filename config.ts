import type { SingBoxConfig } from "./types";
function get_config() {
  const sing_box_config: SingBoxConfig = {
    log: {
      disabled: false,
      output: "./box.log",
      timestamp: true,
      level: "info",
    },
    experimental: {
      clash_api: {
        external_controller: "0.0.0.0:9090",
        external_ui: "dashboard",
      },
    },
    dns: {
      final: "googledns",
      strategy: "prefer_ipv4",
      independent_cache: true,
      servers: [
        {
          type: "udp",
          tag: "googledns",
          server: "8.8.8.8",
          detour: "proxy",
        },
        {
          tag: "local",
          type: "udp",
          server: "223.5.5.5",
        },
      ],
      rules: [
        {
          rule_set: ["chinasite"],
          action: "route",
          server: "local",
        },
      ],
    },
    inbounds: [
      {
        type: "tun",
        tag: "tun-in",
        interface_name: "sing-tun",
        address: ["172.18.0.1/30"],
        mtu: 9000,
        auto_route: true,
        strict_route: true,
        auto_redirect: true,
      },
    ],
    outbounds: [
      {
        type: "selector",
        tag: "proxy",
        outbounds: [],
      },
      { type: "direct", tag: "direct" },
    ],
    route: {
      final: "proxy",
      auto_detect_interface: true,
      default_domain_resolver: "local",
      rules: [
        { action: "sniff" },
        { protocol: "dns", action: "hijack-dns" },
        { ip_is_private: true, action: "route", outbound: "direct" },
        {
          process_path: ["/usr/bin/qbittorrent", "/usr/bin/NetworkManager"],
          action: "route",
          outbound: "direct",
        },
        {
          rule_set: ["chinasite"],
          action: "route",
          outbound: "direct",
        },
      ],
      rule_set: [
        {
          type: "local",
          tag: "chinasite",
          format: "binary",
          path: "chinasite.srs",
        },
      ],
    },
  };
  return sing_box_config;
}

export default get_config;
