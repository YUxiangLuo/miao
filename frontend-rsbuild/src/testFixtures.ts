// 测试夹具工厂：构造满足 API/Clash 类型的 mock，默认值与运行时零值一致。
// 只被 *.test.* 引用；新增必填字段时在这里补默认值，测试只覆盖差异字段。
import type { NodeInfo, RuleInfo, StatusData, SubNodeInfo, SubNodesInfo, SubStatus } from './types/api'
import type { ConnectionGroup, EnrichedConnection } from './types/clash'

export function statusMock(overrides: Partial<StatusData> = {}): StatusData {
  const running = overrides.running ?? false
  const initializing = overrides.initializing ?? false
  const ready = overrides.ready ?? (running && !initializing)
  return {
    running,
    ready,
    phase: overrides.phase ?? (ready ? 'ready' : initializing ? 'initializing' : 'stopped'),
    initializing,
    route_mode: 'rule',
    node_select: 'manual',
    vps_supported: true,
    platform: 'linux',
    mcp: false,
    ...overrides,
    warnings: overrides.warnings ?? [],
  }
}

export function connectionMock(overrides: Partial<EnrichedConnection> = {}): EnrichedConnection {
  return {
    id: 'connection-1',
    upload: 0,
    download: 0,
    uploadSpeed: 0,
    downloadSpeed: 0,
    start: '',
    chains: ['proxy'],
    rule: 'Match',
    rulePayload: '',
    metadata: {},
    ...overrides,
  }
}

export function connectionGroupMock(overrides: Partial<ConnectionGroup> = {}): ConnectionGroup {
  return {
    id: 'group-1',
    domain: 'example.com',
    connections: [],
    downloadSpeed: 0,
    uploadSpeed: 0,
    download: 0,
    upload: 0,
    count: 1,
    rule: 'final',
    ruleLabel: '兜底规则',
    extraRules: 0,
    outbound: 'proxy',
    ...overrides,
  }
}

export function ruleMock(overrides: Partial<RuleInfo> = {}): RuleInfo {
  return { index: 0, skipped: false, raw: '', ...overrides }
}

export function nodeMock(overrides: Partial<NodeInfo> = {}): NodeInfo {
  return { tag: 'node-1', server: 'example.com', server_port: 443, node_type: 'hysteria2', ...overrides }
}

export function subMock(overrides: Partial<SubStatus> = {}): SubStatus {
  return {
    url: 'https://example.com/sub',
    success: true,
    node_count: 0,
    disabled_count: 0,
    state: 'ready',
    ...overrides,
  }
}

export function subNodeMock(overrides: Partial<SubNodeInfo> = {}): SubNodeInfo {
  return {
    name: 'node-1',
    server: 'example.com',
    server_port: 443,
    node_type: 'trojan',
    disabled: false,
    ...overrides,
  }
}

export function subNodesInfoMock(overrides: Partial<SubNodesInfo> = {}): SubNodesInfo {
  return { url: 'https://example.com/sub', nodes: [], stale_disabled: [], ...overrides }
}
