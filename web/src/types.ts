export interface ApiResponse<T> {
  success: boolean;
  message: string;
  data?: T;
}

export interface StatusData {
  running: boolean;
  pid?: number;
  uptime_secs?: number;
}

export interface ConnectivityResult {
  name: string;
  url: string;
  latency_ms?: number;
  success: boolean;
}

export interface SubStatus {
  url: string;
  success: boolean;
  node_count: number;
  error?: string;
}

export interface NodeInfo {
  tag: string;
  server: string;
  server_port: number;
  sni?: string;
  protocol?: string; // Derived/assumed
  source?: string; // 'manual' | 'subscription'
}

export interface LastProxy {
  group: string;
  name: string;
}

export interface VersionInfo {
  current: string;
  latest?: string;
  has_update: boolean;
  download_url?: string;
}
