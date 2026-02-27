export type ConsoleSessionSummary = {
  sessionId: string;
  shell: string;
  createdAt: string;
  lastActivity: string;
  exitCode: number | null;
  clientCount: number;
  cols: number;
  rows: number;
  closed: boolean;
};
