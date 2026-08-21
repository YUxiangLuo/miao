// 测试夹具工厂：构造满足 API/Clash 类型的 mock，默认值与运行时零值一致。
// 只被 *.test.* 引用；新增必填字段时在这里补默认值，测试只覆盖差异字段。
import type { RuleInfo, StatusData } from './types/api'
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
