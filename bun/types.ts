export type DomainSet = {
  rules: [
    {
      domain: string[];
      domain_suffix: string[];
      domain_regex: string[];
    },
  ];
  version: number;
};

export type ClashProxy = {
  type: string;
  name: string;
  [k: string]: any;
};

export type Outbound = {
  type: string;
  tag: string;
  outbounds?: string[];
};
export type Hysteria2 = Outbound & {
  server: string;
  server_port: number;
  password: string;
  up_mbps: number;
  down_mbps: number;
  tls: {
    enabled: boolean;
    server_name: string;
    insecure: boolean;
  };
};
export type Anytls = Outbound & {
  server: string;
  server_port: number;
  password: string;
  tls: {
    enabled: boolean;
    server_name: string;
    insecure: boolean;
  };
};

export type SingBoxConfig = {
  outbounds: Outbound[];
  [k: string]: unknown;
};
