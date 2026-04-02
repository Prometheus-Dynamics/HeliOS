import type { SystemsRuntimeSnapshot } from '$lib/types/systems';

export type ActivityTabId = 'runtime' | 'logs' | 'i2c' | 'imu' | 'console' | 'processes';

export type ActivityTab = {
  id: ActivityTabId;
  label: string;
  detail: string;
};

export function buildActivityTabs(runtime: SystemsRuntimeSnapshot | null | undefined): ActivityTab[] {
  const capabilities = runtime?.capabilities;
  const tabs: ActivityTab[] = [
    { id: 'runtime', label: 'Runtime', detail: 'Platform & policy' },
    { id: 'logs', label: 'Logs', detail: 'Live service output' }
  ];

  if (capabilities?.i2c) {
    tabs.push({ id: 'i2c', label: 'I2C', detail: 'Buses & devices' });
  }
  if (capabilities?.imu) {
    tabs.push({ id: 'imu', label: 'IMU', detail: 'Orientation & axes' });
  }
  if (capabilities?.console ?? true) {
    tabs.push({ id: 'console', label: 'Console', detail: 'Runtime shell' });
  }
  if (capabilities?.processes ?? true) {
    tabs.push({ id: 'processes', label: 'Processes', detail: 'CPU & memory by program' });
  }

  return tabs;
}
