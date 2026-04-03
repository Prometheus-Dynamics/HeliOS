/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { LogSourceKind } from './LogSourceKind';
import type { SystemdUnitStatus } from './SystemdUnitStatus';
export type LogSource = {
    group: string;
    id: string;
    important: boolean;
    kind: LogSourceKind;
    label: string;
    path?: string | null;
    status?: (null | SystemdUnitStatus);
    unit?: string | null;
};

