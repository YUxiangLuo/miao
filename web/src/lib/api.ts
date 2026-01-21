import type { ApiResponse, StatusData, SubStatus, NodeInfo, ConnectivityResult, VersionInfo } from '../types';

const API_BASE = '/api';

async function fetchApi<T>(endpoint: string, options?: RequestInit): Promise<ApiResponse<T>> {
  try {
    const res = await fetch(`${API_BASE}${endpoint}`, options);
    const data = await res.json();
    return data;
  } catch (error) {
    return { success: false, message: error instanceof Error ? error.message : 'Unknown error' };
  }
}

export const api = {
  getStatus: () => fetchApi<StatusData>('/status'),
  startService: () => fetchApi<void>('/service/start', { method: 'POST' }),
  stopService: () => fetchApi<void>('/service/stop', { method: 'POST' }),
  
  testConnectivity: (url: string) => fetchApi<ConnectivityResult>('/connectivity', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ url })
  }),

  getSubs: () => fetchApi<SubStatus[]>('/subs'),
  addSub: (url: string) => fetchApi<void>('/subs', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ url })
  }),
  deleteSub: (url: string) => fetchApi<void>('/subs', {
    method: 'DELETE',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ url })
  }),
  refreshSubs: () => fetchApi<void>('/subs/refresh', { method: 'POST' }),

  getNodes: () => fetchApi<NodeInfo[]>('/nodes'),
  addNode: (node: any) => fetchApi<void>('/nodes', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(node)
  }),
  deleteNode: (tag: string) => fetchApi<void>('/nodes', {
    method: 'DELETE',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ tag })
  }),

  getVersion: () => fetchApi<VersionInfo>('/version'),
  upgrade: () => fetchApi<string>('/upgrade', { method: 'POST' }),

  setLastProxy: (group: string, name: string) => fetchApi<void>('/last-proxy', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ group, name })
  }),
};
