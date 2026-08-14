import { Play, Square, Wifi } from 'lucide-react'
import { Button, SectionCard } from './ui.jsx'
import { classNames, CONNECTIVITY_SITES, getDelayTone } from '../utils.js'

export function ConnectivityCard({ 
  connectivityResults, 
  testingConnectivity, 
  currentTestingSite,
  status,
  onTestAll, 
  onStopTest,
  onTestSingleSite 
}) {
  return (
    <SectionCard
      bodyClassName="panel-body-tight"
      header={
        <div className="section-header">
          <div className="section-title-wrap">
            <Wifi size={14} className="section-icon" />
            <span>连通性测试</span>
          </div>
          <Button 
            tone="secondary" 
            size="sm" 
            icon={testingConnectivity ? <Square size={11} /> : <Play size={11} />} 
            disabled={status.initializing} 
            onClick={testingConnectivity ? onStopTest : onTestAll}
          >
            {testingConnectivity ? '停止测试' : '开始测试'}
          </Button>
        </div>
      }
    >
      <div className="connectivity-grid">
        {CONNECTIVITY_SITES.map((site) => {
          const result = connectivityResults[site.name]
          const tone = result ? (result.success ? getDelayTone(result.latency_ms) : 'timeout') : ''
          const isTesting = currentTestingSite === site.name
          return (
            <button 
              key={site.name} 
              className={classNames('connectivity-item', tone, isTesting && 'testing')} 
              onClick={() => !currentTestingSite && onTestSingleSite(site)} 
              disabled={Boolean(currentTestingSite)}
            >
              <div className="connectivity-copy">
                <span>{site.name}</span>
                <span>{result ? (result.success ? `${result.latency_ms}ms` : '超时') : '--'}</span>
              </div>
            </button>
          )
        })}
      </div>
    </SectionCard>
  )
}
