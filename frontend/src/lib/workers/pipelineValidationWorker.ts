type PlannerDiagnostic = {
  code: string;
  message: string;
  span?: { node?: string | null; port?: string | null } | null;
};

type Payload = {
  requestId: number;
  diagnostics: PlannerDiagnostic[];
};

type ValidationResponse = {
  requestId: number;
  warnings: string[];
};

self.onmessage = (event: MessageEvent<Payload>) => {
  const { requestId, diagnostics } = event.data;
  const list = Array.isArray(diagnostics) ? diagnostics : [];
  const warnings = list.map((diag) => {
    const nodeId = diag?.span?.node ?? null;
    const port = diag?.span?.port ?? null;
    const at = nodeId ? ` (node ${nodeId}${port ? `:${port}` : ''})` : '';
    return `${diag.code}: ${diag.message}${at}`;
  });
  const response: ValidationResponse = { requestId, warnings };
  self.postMessage(response);
};

export {};
