import { useEffect, useState } from 'react';
import { FlaskConical } from 'lucide-react';
import { gahApi } from '../api/client.js';
import { useWebSocket } from '../ws/WebSocketContext.js';

/** Sample state must stay visibly distinct from a connected factory on every page. */
export function SampleDataBanner() {
  const [sample, setSample] = useState(false);
  const { reconnectSeq } = useWebSocket();
  useEffect(() => {
    let current = true;
    void gahApi.getCoordinatorInfo()
      .then(info => { if (current) setSample(info.environment === 'mock'); })
      .catch(() => { /* An unavailable identity never proves this is a live factory. */ });
    return () => { current = false; };
  }, [reconnectSeq]);
  if (!sample) return null;
  return (
    <aside aria-label="Sample data environment" className="flex shrink-0 items-start gap-2 border-b border-subtle bg-raised px-4 py-2 text-sm text-secondary">
      <FlaskConical size={16} className="mt-0.5 shrink-0 text-warning" aria-hidden="true" />
      <p><strong className="text-warning">Sample data.</strong> No live factory is connected. Work, usage, and update notices are simulated.</p>
    </aside>
  );
}
