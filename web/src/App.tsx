import { useEffect, useState, useRef } from 'react';
import { Activity, LayoutGrid, Settings, Download, Globe, StopCircle, Youtube, Github, Twitter, Bot } from 'lucide-react';
import { StatusCard } from './components/StatusCard';
import { TrafficChart } from './components/TrafficChart';
import { NodeList } from './components/NodeList';
import { SubManager } from './components/SubManager';
import { NodeManager } from './components/NodeManager';
import { api } from './lib/api';
import type { StatusData, SubStatus, NodeInfo, VersionInfo } from './types';
import { clsx } from 'clsx';

const GoogleIcon = ({ size, className }: { size?: number, className?: string }) => (
  <svg 
    width={size || 24} 
    height={size || 24} 
    viewBox="0 0 24 24" 
    xmlns="http://www.w3.org/2000/svg"
    className={className}
    fill="currentColor"
  >
    <path d="M21.35 11.1h-9.17v2.73h6.51c-.33 3.81-3.5 5.44-6.5 5.44C8.36 19.27 5 16.25 5 12c0-4.1 3.2-7.27 7.2-7.27 3.09 0 4.9 1.97 4.9 1.97L19 4.72S16.56 2 12.1 2C6.42 2 2.03 6.8 2.03 12c0 5.05 4.13 10 10.22 10 5.35 0 9.25-3.67 9.25-9.09 0-1.15-.15-1.81-.15-1.81Z" />
  </svg>
);

const SITES = [
  { id: 'google', name: 'Google', url: 'https://www.google.com', icon: GoogleIcon },
  { id: 'youtube', name: 'YouTube', url: 'https://www.youtube.com', icon: Youtube },
  { id: 'github', name: 'GitHub', url: 'https://github.com', icon: Github },
  { id: 'openai', name: 'OpenAI', url: 'https://api.openai.com', icon: Bot },
  { id: 'twitter', name: 'Twitter', url: 'https://twitter.com', icon: Twitter },
];

function App() {
  const [activeTab, setActiveTab] = useState<'dashboard' | 'proxies' | 'config'>('dashboard');
  const [status, setStatus] = useState<StatusData | null>(null);
  const [loadingStatus, setLoadingStatus] = useState(false);
  const [toggling, setToggling] = useState(false);
  
  const [subs, setSubs] = useState<SubStatus[]>([]);
  const [nodes, setNodes] = useState<NodeInfo[]>([]);
  
  const [selectedNode, setSelectedNode] = useState<string>('');
  const [latencies, setLatencies] = useState<Record<string, number>>({});
  
  // New Site Connectivity State
  const [siteLatencies, setSiteLatencies] = useState<Record<string, number | null>>({});
  const [testingSites, setTestingSites] = useState<Record<string, boolean>>({});

  const [testingSpeed, setTestingSpeed] = useState(false);
  const [testingNodes, setTestingNodes] = useState<string[]>([]);
  const [upgrading, setUpgrading] = useState(false);
  const abortControllerRef = useRef<AbortController | null>(null);
  const statusRunningRef = useRef(false);
  
  const [version, setVersion] = useState<VersionInfo | null>(null);

  // Sync ref with state
  useEffect(() => {
    statusRunningRef.current = Boolean(status?.running);
  }, [status?.running]);

  // Initial Data Load
  useEffect(() => {
    fetchStatus();
    fetchSubs();
    fetchNodes();
    fetchVersion();
  }, []);

  // Interval updates
  useEffect(() => {
    const interval = setInterval(() => {
      fetchStatus();
      if (statusRunningRef.current) {
        fetchClashData(); 
      }
    }, 3000);
    return () => clearInterval(interval);
  }, []);

  const handleUpgrade = async () => {
    if (!confirm(`Upgrade to version ${version?.latest}? The service will restart.`)) return;
    
    setUpgrading(true);
    try {
      const res = await api.upgrade();
      if (res.success) {
        alert('Upgrade successful! The service is restarting. The page will reload in 10 seconds.');
        setTimeout(() => window.location.reload(), 10000);
      } else {
        alert(`Upgrade failed: ${res.message}`);
        setUpgrading(false);
      }
    } catch (e) {
      alert('Upgrade request failed. Check logs.');
      setUpgrading(false);
    }
  };

  const fetchStatus = async () => {
    // Only set loading if status is null (first load) to avoid flickering
    if (!status) setLoadingStatus(true);
    const res = await api.getStatus();
    setLoadingStatus(false);
    if (res.success && res.data) {
      setStatus(res.data);
    }
  };

  const fetchClashData = async () => {
    try {
      const res = await fetch(`http://${window.location.hostname}:6262/proxies/proxy`);
      if (res.ok) {
        const data = await res.json();
        if (data.now) {
          setSelectedNode(data.now);
        }
      }
    } catch (e) {
      // ignore errors if service stopped
    }
  };

  const fetchSubs = async () => {
    const res = await api.getSubs();
    if (res.success && res.data) setSubs(res.data);
  };

  const fetchNodes = async () => {
    const res = await api.getNodes();
    if (res.success && res.data) setNodes(res.data);
  };

  const fetchVersion = async () => {
    const res = await api.getVersion();
    if (res.success && res.data) setVersion(res.data);
  };

  const handleToggleService = async () => {
    setToggling(true);
    if (status?.running) {
      await api.stopService();
    } else {
      await api.startService();
    }
    await fetchStatus();
    setToggling(false);
  };

  const handleNodeSelect = async (tag: string) => {
    setSelectedNode(tag);
    // Persist to backend
    await api.setLastProxy('proxy', tag); 
    
    // Apply via Clash API
    try {
      await fetch(`http://${window.location.hostname}:6262/proxies/proxy`, {
        method: 'PUT',
        headers: {'Content-Type': 'application/json'},
        body: JSON.stringify({ name: tag })
      });
      fetchClashData(); // sync immediately
    } catch (e) {
      console.error("Failed to set proxy via Clash API", e);
    }
  };

  const testSiteLatency = async (id: string, url: string) => {
    setTestingSites(prev => ({ ...prev, [id]: true }));
    setSiteLatencies(prev => ({ ...prev, [id]: null }));
    
    const res = await api.testConnectivity(url);
    
    setTestingSites(prev => ({ ...prev, [id]: false }));
    
    if (res.success && res.data && res.data.latency_ms !== undefined) {
      const latency = res.data.latency_ms;
      setSiteLatencies(prev => ({ ...prev, [id]: latency }));
    } else {
      setSiteLatencies(prev => ({ ...prev, [id]: -1 }));
    }
  };

  const testAllSites = () => {
    SITES.forEach(site => testSiteLatency(site.id, site.url));
  };

  const testNodeLatency = async (tag: string) => {
    setTestingNodes(prev => [...prev, tag]);
    try {
      const url = `http://${window.location.hostname}:6262/proxies/${encodeURIComponent(tag)}/delay?timeout=5000&url=http://www.gstatic.com/generate_204`;
      const res = await fetch(url);
      if (res.ok) {
        const data = await res.json();
        setLatencies(prev => ({ ...prev, [tag]: data.delay || -1 }));
      } else {
        setLatencies(prev => ({ ...prev, [tag]: -1 }));
      }
    } catch {
      setLatencies(prev => ({ ...prev, [tag]: -1 }));
    } finally {
      setTestingNodes(prev => prev.filter(t => t !== tag));
    }
  };

  const toggleSpeedTest = async () => {
    if (testingSpeed) {
      abortControllerRef.current?.abort();
      setTestingSpeed(false);
      setTestingNodes([]);
      return;
    }

    setTestingSpeed(true);
    setLatencies({}); // Clear previous results
    abortControllerRef.current = new AbortController();
    const signal = abortControllerRef.current.signal;
    
    // Mark all as testing initially
    setTestingNodes(nodes.map(n => n.tag));
    
    const newLatencies: Record<string, number> = {};
    const batchSize = 5;
    
    try {
      for (let i = 0; i < nodes.length; i += batchSize) {
        if (signal.aborted) break;
        
        const batch = nodes.slice(i, i + batchSize);
        await Promise.all(batch.map(async (node) => {
          try {
            const url = `http://${window.location.hostname}:6262/proxies/${encodeURIComponent(node.tag)}/delay?timeout=5000&url=http://www.gstatic.com/generate_204`;
            const res = await fetch(url, { signal });
            if (res.ok) {
              const data = await res.json();
              newLatencies[node.tag] = data.delay || -1;
            } else {
               newLatencies[node.tag] = -1;
            }
          } catch (e) {
            if (!signal.aborted) newLatencies[node.tag] = -1;
          }
        }));
        
        if (!signal.aborted) {
          setLatencies(prev => ({ ...prev, ...newLatencies }));
          // Remove finished batch from testingNodes
          const finishedTags = batch.map(n => n.tag);
          setTestingNodes(prev => prev.filter(t => !finishedTags.includes(t)));
        }
      }
    } finally {
      if (!signal.aborted) {
        setTestingSpeed(false);
        setTestingNodes([]);
      }
    }
  };

  return (
    <div className="min-h-screen bg-miao-bg text-miao-text p-4 md:p-8 font-sans selection:bg-miao-green selection:text-white">
      <div className="max-w-7xl mx-auto space-y-8">
        
        {/* Header */}
        <header className="flex flex-col md:flex-row md:items-center justify-between gap-4">
          <div 
            className="flex items-center gap-3 cursor-pointer group" 
            onClick={() => setActiveTab('dashboard')}
            title="Go to Dashboard"
          >
            <div className="w-10 h-10 bg-gradient-to-br from-miao-green to-emerald-600 rounded-xl flex items-center justify-center text-white font-bold text-xl shadow-lg shadow-miao-green/20 group-hover:scale-105 transition-transform">
              M
            </div>
            <div>
              <h1 className="text-2xl font-bold tracking-tight group-hover:text-miao-green transition-colors">Miao Dashboard</h1>
              <p className="text-miao-muted text-sm flex items-center gap-2">
                {version?.current || "..."} 
                {version?.has_update && (
                  <span className="text-miao-green cursor-pointer hover:underline flex items-center gap-1" onClick={handleUpgrade}>
                    <Download size={12} /> Update Available ({version.latest})
                  </span>
                )}
              </p>
            </div>
          </div>

          <div className="flex bg-miao-panel p-1 rounded-lg border border-miao-border">
            {[
              { id: 'dashboard', icon: Activity, label: 'Overview' },
              { id: 'proxies', icon: LayoutGrid, label: 'Proxies' },
              { id: 'config', icon: Settings, label: 'Config' },
            ].map((tab) => (
              <button
                key={tab.id}
                onClick={() => setActiveTab(tab.id as any)}
                className={clsx(
                  "px-4 py-2 rounded-md flex items-center gap-2 text-sm font-medium transition-all",
                  activeTab === tab.id 
                    ? "bg-miao-bg text-miao-text shadow-sm ring-1 ring-miao-border" 
                    : "text-miao-muted hover:text-miao-text hover:bg-white/5"
                )}
              >
                <tab.icon size={16} />
                {tab.label}
              </button>
            ))}
          </div>
        </header>

        {/* Content */}
        <main className="space-y-6">
          {activeTab === 'dashboard' && (
            <div className="grid grid-cols-1 lg:grid-cols-3 gap-6">
              <div className="lg:col-span-1 space-y-6">
                <StatusCard 
                  status={status} 
                  loading={loadingStatus} 
                  onToggle={handleToggleService}
                  toggling={toggling}
                />
                
                <div className="bg-miao-panel border border-miao-border rounded-xl p-6 flex flex-col justify-center relative overflow-hidden">
                  <h2 className="text-miao-muted text-sm font-medium uppercase tracking-wider mb-2">Current Node</h2>
                  <div className="flex items-center gap-3">
                    <Globe size={24} className="text-miao-green" />
                    <span className="text-xl font-bold text-miao-text truncate" title={selectedNode}>
                      {selectedNode || "Direct / None"}
                    </span>
                  </div>
                </div>

                <div className="bg-miao-panel border border-miao-border rounded-xl p-6 flex flex-col justify-center relative overflow-hidden">
                  <h2 className="text-miao-muted text-sm font-medium uppercase tracking-wider mb-2">System Update</h2>
                  <div className="flex items-center justify-between">
                    <div>
                      <div className="text-xl font-bold text-miao-text">{version?.current}</div>
                      {version?.has_update ? (
                        <div className="text-xs text-miao-green mt-1">Latest: {version.latest}</div>
                      ) : (
                        <div className="text-xs text-miao-muted mt-1">Up to date</div>
                      )}
                    </div>
                    {version?.has_update && (
                      <button 
                        onClick={handleUpgrade}
                        disabled={upgrading}
                        className="px-4 py-2 bg-miao-green hover:bg-miao-green-hover text-white rounded-lg text-sm font-medium transition-all flex items-center gap-2 disabled:opacity-50"
                      >
                        {upgrading ? <Download size={16} className="animate-bounce" /> : <Download size={16} />}
                        {upgrading ? "Updating..." : "Upgrade"}
                      </button>
                    )}
                  </div>
                </div>
              </div>
              
              <div className="lg:col-span-2 h-64 lg:h-auto">
                <TrafficChart />
              </div>
              
              {/* Site Connectivity Check */}
              <div className="lg:col-span-3 bg-miao-panel border border-miao-border rounded-xl p-6">
                <div className="flex items-center justify-between mb-4">
                  <div>
                    <h3 className="font-medium text-lg">Connectivity Check</h3>
                    <p className="text-miao-muted text-sm">Test connection latency to popular sites.</p>
                  </div>
                  <button 
                    onClick={testAllSites}
                    className="px-4 py-2 bg-miao-bg border border-miao-border hover:border-miao-green text-miao-text rounded-lg transition-all text-sm font-medium"
                  >
                    Test All
                  </button>
                </div>
                
                <div className="grid grid-cols-1 sm:grid-cols-2 md:grid-cols-3 lg:grid-cols-5 gap-3">
                  {SITES.map((site) => {
                    const latency = siteLatencies[site.id];
                    const loading = testingSites[site.id];
                    
                    return (
                      <button 
                        key={site.id}
                        onClick={() => testSiteLatency(site.id, site.url)}
                        disabled={loading}
                        className="flex items-center gap-3 px-3 py-2.5 bg-miao-bg/50 border border-miao-border hover:border-miao-green/50 hover:bg-miao-bg rounded-xl transition-all group text-left"
                      >
                        <site.icon size={20} className="text-miao-muted group-hover:text-miao-text shrink-0 transition-colors" />
                        
                        <div className="flex flex-col min-w-0">
                          <span className="font-medium text-sm text-miao-text leading-tight">{site.name}</span>
                          
                          {loading ? (
                            <span className="text-[10px] text-miao-muted animate-pulse">Testing...</span>
                          ) : latency !== undefined && latency !== null ? (
                            <span className={clsx("text-xs font-mono font-bold", 
                              latency === -1 ? "text-miao-red" : 
                              latency < 200 ? "text-miao-green" : "text-yellow-500"
                            )}>
                              {latency === -1 ? "Timeout" : `${latency}ms`}
                            </span>
                          ) : (
                            <span className="text-[10px] text-miao-muted group-hover:text-miao-green transition-colors">Click to test</span>
                          )}
                        </div>
                      </button>
                    );
                  })}
                </div>
              </div>
            </div>
          )}

          {activeTab === 'proxies' && (
            <div className="space-y-6">
              <div className="flex items-center justify-between">
                <h2 className="text-xl font-bold">Proxy Nodes</h2>
                <div className="flex items-center gap-3">
                   <div className="flex gap-2 text-sm mr-2">
                     <span className="px-3 py-1 rounded-full bg-miao-green/10 text-miao-green border border-miao-green/20">
                       {nodes.length} Nodes
                     </span>
                   </div>
                   <button 
                     onClick={toggleSpeedTest}
                     disabled={!status?.running || (!testingSpeed && testingNodes.length > 0)}
                     className={clsx(
                       "px-4 py-2 border rounded-lg text-sm font-medium transition-all flex items-center gap-2 disabled:opacity-50 disabled:cursor-not-allowed",
                       testingSpeed 
                         ? "bg-miao-red/10 border-miao-red text-miao-red hover:bg-miao-red/20" 
                         : "bg-miao-panel border-miao-border hover:border-miao-green text-miao-text"
                     )}
                   >
                     {testingSpeed ? <StopCircle size={14} /> : <Globe size={14} />}
                     {testingSpeed ? "Stop Test" : "Test Speed"}
                   </button>
                </div>
              </div>
              
              {/* Node List */}
              <NodeList 
                nodes={nodes} 
                selectedTag={selectedNode} 
                onSelect={handleNodeSelect}
                latencies={latencies}
                testingNodes={testingNodes}
                onTest={testNodeLatency}
              />
            </div>
          )}

          {activeTab === 'config' && (
            <div className="grid grid-cols-1 lg:grid-cols-2 gap-8 h-[calc(100vh-140px)]">
              <SubManager subs={subs} onUpdate={() => { fetchSubs(); fetchNodes(); }} />
              <NodeManager nodes={nodes} onUpdate={() => { fetchNodes(); }} />
            </div>
          )}
        </main>
      </div>
    </div>
  );
}

export default App;