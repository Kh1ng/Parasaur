import type { ReportData, SettingsConfigProfileSummary } from '@git-agent-harness/contracts';
import { gahApi } from '../api/client.js';
import { useGahStore } from './gahStore.js';

jest.mock('../api/client.js', () => ({
  gahApi: { getReport: jest.fn(), getProfileConfig: jest.fn() },
  GahApiError: class GahApiError extends Error {},
}));

const getReport = jest.mocked(gahApi.getReport);
const getProfileConfig = jest.mocked(gahApi.getProfileConfig);
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
  jest.useFakeTimers().setSystemTime(new Date('2026-01-01T00:00:00Z'));
});
afterEach(() => jest.useRealTimers());

const config = (profile: string) => ({ profile, backend_instances: [] }) as unknown as SettingsConfigProfileSummary;

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
