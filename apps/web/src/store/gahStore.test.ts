import type { ControllerEvent, DoctorSnapshot, LedgerEntry, ReportData, ReportSeriesData, SettingsConfigProfileSummary } from '@git-agent-harness/contracts';
import { gahApi } from '../api/client.js';
import type { LoopStatus, StartLoopResult, StopLoopResult } from '../api/client.js';
import { useGahStore, workTimelineKey } from './gahStore.js';

jest.mock('../api/client.js', () => ({
  gahApi: { getReport: jest.fn(), getProfileConfig: jest.fn(), getWorkTimeline: jest.fn(), getEvents: jest.fn(), getDoctor: jest.fn(), getReportSeries: jest.fn(), getLoopStatus: jest.fn(), startLoop: jest.fn(), stopLoop: jest.fn() },
  GahApiError: class GahApiError extends Error {},
}));

const getReport = jest.mocked(gahApi.getReport);
const getProfileConfig = jest.mocked(gahApi.getProfileConfig);
const getWorkTimeline = jest.mocked(gahApi.getWorkTimeline);
const getEvents = jest.mocked(gahApi.getEvents);
const initialState = useGahStore.getState();
const report = (profile: string, total_entries = 1): ReportData => ({
  ledger_path: '/ledger', total_entries, since: '', profile, group_by: 'backend', comparisons: [], trend: [],
});
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}

beforeEach(() => {
  useGahStore.setState(initialState, true);
  getReport.mockReset();
  getProfileConfig.mockReset();
  getWorkTimeline.mockReset();
  getEvents.mockReset();
  jest.mocked(gahApi.getDoctor).mockReset();
  jest.mocked(gahApi.getReportSeries).mockReset();
  jest.mocked(gahApi.getLoopStatus).mockReset();
  jest.mocked(gahApi.startLoop).mockReset();
  jest.mocked(gahApi.stopLoop).mockReset();
  jest.useFakeTimers().setSystemTime(new Date('2026-01-01T00:00:00Z'));
});
afterEach(() => jest.useRealTimers());

const config = (profile: string) => ({ profile, backend_instances: [] }) as unknown as SettingsConfigProfileSummary;

describe.each(['readiness', 'usage trend', 'controller'] as const)('%s project isolation', (resource) => {
  const fetchResource = (profile: string) => resource === 'readiness'
    ? useGahStore.getState().fetchDoctor(profile)
    : resource === 'controller' ? useGahStore.getState().fetchLoopStatus(profile)
    : useGahStore.getState().fetchReportSeries({ profile, since: '14d', bucket: 'daily' });
  const readResource = () => resource === 'readiness' ? useGahStore.getState().doctor : resource === 'controller' ? useGahStore.getState().loopStatus : useGahStore.getState().reportSeries;
  const mockRequest = (promise: Promise<unknown>) => {
    if (resource === 'readiness') jest.mocked(gahApi.getDoctor).mockReturnValueOnce(promise as Promise<DoctorSnapshot>);
    else if (resource === 'controller') jest.mocked(gahApi.getLoopStatus).mockReturnValueOnce(promise as Promise<LoopStatus>);
    else jest.mocked(gahApi.getReportSeries).mockReturnValueOnce(promise as Promise<ReportSeriesData>);
  };

  test('a new project starts immediately and a late old result cannot overwrite it', async () => {
    const old = deferred<unknown>();
    const latest = deferred<unknown>();
    mockRequest(old.promise);
    mockRequest(latest.promise);
    const first = fetchResource('alpha');
    const second = fetchResource('beta');
    expect(readResource()).toMatchObject({ data: null, loading: true });
    const beta = resource === 'readiness'
      ? { schema_version: 1, generated_at: '', overall_status: 'ok', checks: [] }
      : resource === 'controller' ? { running: true, pid: 42 }
      : { ledger_path: '/beta', profile: 'beta', since: '14d', bucket: 'daily', series: [] };
    latest.resolve(beta);
    await second;
    expect(readResource().data).toEqual(beta);
    old.resolve({ stale: true });
    await first;
    expect(readResource().data).toEqual(beta);
  });

  test('a failed project switch clears the previous result', async () => {
    mockRequest(Promise.resolve({ old: true }));
    await fetchResource('alpha');
    const failure = deferred<unknown>();
    mockRequest(failure.promise.then(() => { throw new Error('beta unavailable'); }));
    const second = fetchResource('beta');
    expect(readResource().data).toBeNull();
    failure.resolve(null);
    await second;
    expect(readResource()).toMatchObject({ data: null, loading: false, error: 'beta unavailable' });
  });
});

test.each(['start', 'stop'] as const)('completing %s on a previous project cannot restore its controller status', async (action) => {
  jest.mocked(gahApi.getLoopStatus).mockResolvedValue({ running: false });
  await useGahStore.getState().fetchLoopStatus('alpha');
  const mutation = deferred<StartLoopResult & StopLoopResult>();
  jest.mocked(gahApi.startLoop).mockReturnValue(mutation.promise);
  jest.mocked(gahApi.stopLoop).mockReturnValue(mutation.promise);
  const pending = action === 'start' ? useGahStore.getState().startLoop('alpha') : useGahStore.getState().stopLoop('alpha');
  await useGahStore.getState().fetchLoopStatus('beta');
  mutation.resolve({ started: true, stopped: true });
  await pending;
  expect(jest.mocked(gahApi.getLoopStatus).mock.calls).toEqual([['alpha'], ['beta']]);
  expect(useGahStore.getState().loopStatus.key).toBe('beta');
});

test('a rapid project round trip cannot restore an older account configuration', async () => {
  const old = deferred<SettingsConfigProfileSummary>();
  const other = deferred<SettingsConfigProfileSummary>();
  const fresh = deferred<SettingsConfigProfileSummary>();
  getProfileConfig.mockReturnValueOnce(old.promise).mockReturnValueOnce(other.promise).mockReturnValueOnce(fresh.promise);
  const first = useGahStore.getState().fetchProfileConfig('alpha');
  const second = useGahStore.getState().fetchProfileConfig('beta');
  const third = useGahStore.getState().fetchProfileConfig('alpha');
  fresh.resolve({ ...config('alpha'), max_fix_attempts_per_mr: 9 });
  await third;
  old.resolve({ ...config('alpha'), max_fix_attempts_per_mr: 1 });
  other.resolve(config('beta'));
  await Promise.all([first, second]);
  expect(useGahStore.getState().profileConfig.data?.max_fix_attempts_per_mr).toBe(9);
});

test('a project switch fetches its own controller decisions while another read is in flight', async () => {
  const alpha = deferred<ControllerEvent[]>();
  const beta = deferred<ControllerEvent[]>();
  getEvents.mockReturnValueOnce(alpha.promise).mockReturnValueOnce(beta.promise);
  const first = useGahStore.getState().fetchEvents({ profile: 'alpha' });
  const second = useGahStore.getState().fetchEvents({ profile: 'beta' });
  expect(getEvents).toHaveBeenCalledTimes(2);
  beta.resolve([{ profile: 'beta' }] as ControllerEvent[]);
  await second;
  alpha.resolve([{ profile: 'alpha' }] as ControllerEvent[]);
  await first;
  expect(useGahStore.getState().events.data?.[0].profile).toBe('beta');
});

test('the same issue number has independent cached history in each project', async () => {
  const alpha = deferred<LedgerEntry[]>();
  const beta = deferred<LedgerEntry[]>();
  getWorkTimeline.mockReturnValueOnce(alpha.promise).mockReturnValueOnce(beta.promise);
  const first = useGahStore.getState().fetchWorkTimeline('#42', { profile: 'alpha' });
  const second = useGahStore.getState().fetchWorkTimeline('#42', { profile: 'beta' });
  beta.resolve([{ profile: 'beta' }] as LedgerEntry[]);
  await second;
  alpha.resolve([{ profile: 'alpha' }] as LedgerEntry[]);
  await first;
  const timelines = useGahStore.getState().workTimelines;
  expect(timelines[workTimelineKey('#42', 'alpha')].data?.[0].profile).toBe('alpha');
  expect(timelines[workTimelineKey('#42', 'beta')].data?.[0].profile).toBe('beta');
  expect(getWorkTimeline).toHaveBeenNthCalledWith(2, '#42', 'beta');
});

test('switching projects clears the previous account configuration before a request completes', async () => {
  getProfileConfig.mockResolvedValueOnce(config('alpha'));
  await useGahStore.getState().fetchProfileConfig('alpha');
  const request = deferred<SettingsConfigProfileSummary>();
  getProfileConfig.mockReturnValue(request.promise);
  const pending = useGahStore.getState().fetchProfileConfig('beta');
  expect(useGahStore.getState().profileConfig).toMatchObject({ key: 'beta', loading: true, data: null, fetchedAt: null });
  request.resolve(config('beta'));
  await pending;
  expect(useGahStore.getState().profileConfig.data?.profile).toBe('beta');
});

test('a failed project switch never leaves another project account configuration visible', async () => {
  getProfileConfig.mockResolvedValueOnce(config('alpha')).mockRejectedValueOnce(new Error('beta unavailable'));
  await useGahStore.getState().fetchProfileConfig('alpha');
  await useGahStore.getState().fetchProfileConfig('beta');
  expect(useGahStore.getState().profileConfig).toMatchObject({ key: 'beta', data: null, loading: false, error: 'beta unavailable' });
});

test('refreshing the same project retains its own configuration while the request is pending', async () => {
  getProfileConfig.mockResolvedValueOnce(config('alpha'));
  await useGahStore.getState().fetchProfileConfig('alpha');
  const request = deferred<SettingsConfigProfileSummary>();
  getProfileConfig.mockReturnValue(request.promise);
  const pending = useGahStore.getState().fetchProfileConfig('alpha', { force: true });
  expect(useGahStore.getState().profileConfig.data?.profile).toBe('alpha');
  request.resolve(config('alpha'));
  await pending;
});

test('two readers share the fetched report with one API call', async () => {
  const data = report('alpha');
  getReport.mockResolvedValue(data);
  await useGahStore.getState().fetchReport({ profile: 'alpha' });
  const firstRead = useGahStore.getState().report.data;
  await useGahStore.getState().fetchReport({ profile: 'alpha' });
  expect(firstRead).toEqual(data);
  expect(useGahStore.getState().report.data).toBe(firstRead);
  expect(getReport).toHaveBeenCalledTimes(1);
  expect(getReport).toHaveBeenCalledWith({ profile: 'alpha' });
});

test('concurrent readers make one request and see its result', async () => {
  const request = deferred<ReportData>();
  getReport.mockReturnValue(request.promise);
  const first = useGahStore.getState().fetchReport({ profile: 'alpha' });
  const second = useGahStore.getState().fetchReport({ profile: 'alpha' });
  expect(getReport).toHaveBeenCalledTimes(1);
  expect(useGahStore.getState().report.loading).toBe(true);
  request.resolve(report('alpha'));
  await Promise.all([first, second]);
  expect(useGahStore.getState().report).toMatchObject({ data: report('alpha'), loading: false, error: null });
});

test('refresh fetches again even while the previous result is fresh', async () => {
  getReport.mockResolvedValueOnce(report('alpha')).mockResolvedValueOnce(report('alpha', 2));
  await useGahStore.getState().fetchReport({ profile: 'alpha' });
  await useGahStore.getState().fetchReport({ profile: 'alpha' }, { force: true });
  expect(getReport).toHaveBeenCalledTimes(2);
  expect(useGahStore.getState().report.data).toEqual(report('alpha', 2));
});

test('a failed profile fetch exposes the error and clears the previous profile data', async () => {
  getReport.mockResolvedValueOnce(report('alpha')).mockRejectedValueOnce(new Error('provider unavailable'));
  await useGahStore.getState().fetchReport({ profile: 'alpha' });
  await useGahStore.getState().fetchReport({ profile: 'beta' });
  expect(getReport).toHaveBeenCalledTimes(2);
  expect(useGahStore.getState().report).toMatchObject({ data: null, loading: false, error: 'provider unavailable', fetchedAt: null });
});

test('a late response for the previous profile cannot overwrite the newer result', async () => {
  const oldRequest = deferred<ReportData>();
  const newRequest = deferred<ReportData>();
  getReport.mockReturnValueOnce(oldRequest.promise).mockReturnValueOnce(newRequest.promise);
  const oldFetch = useGahStore.getState().fetchReport({ profile: 'alpha' });
  const newFetch = useGahStore.getState().fetchReport({ profile: 'beta' });
  expect(getReport).toHaveBeenCalledTimes(2);
  newRequest.resolve(report('beta'));
  await newFetch;
  oldRequest.resolve(report('alpha'));
  await oldFetch;
  expect(useGahStore.getState().report).toMatchObject({ data: report('beta'), loading: false, error: null });
});
